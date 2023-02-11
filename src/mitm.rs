//! Module for the Redirector server which handles redirecting the clients
//! to the correct address for the main server.

use blaze_pk::packet::{Packet, PacketDebug};
use blaze_ssl_async::stream::BlazeStream;
use log::{debug, error, info};
use std::{io, sync::Arc};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    select,
};

use crate::{components::Components, retriever::Retriever, MAIN_PORT};

/// Starts the MITM server. This server is responsible for creating a sort of
/// proxy between this server and the official servers. All packets send and
/// recieved by this server are forwarded to the official servers and are logged
/// using the debug logging.
pub async fn start_server(retriever: Arc<Retriever>) {
    // Initializing the underlying TCP listener
    let listener = {
        let port = MAIN_PORT;
        match TcpListener::bind(("0.0.0.0", port)).await {
            Ok(value) => {
                info!("Started MITM server (Port: {})", port);
                value
            }
            Err(_) => {
                error!("Failed to bind MITM server (Port: {})", port);
                panic!()
            }
        }
    };

    // Accept incoming connections
    loop {
        let (stream, addr) = match listener.accept().await {
            Ok(value) => value,
            Err(err) => {
                error!("Failed to accept MITM connection: {err:?}");
                continue;
            }
        };
        let retriever = retriever.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_client(stream, retriever).await {
                error!("Unable to handle MITM (Addr: {addr}): {err}");
            }
        });
    }
}

/// Handles dealing with a redirector client
///
/// `stream`   The stream to the client
/// `addr`     The client address
/// `instance` The server instance information
/// `shutdown` Async safely shutdown reciever
async fn handle_client(mut client: TcpStream, retriever: Arc<Retriever>) -> io::Result<()> {
    let mut server = match retriever.stream().await {
        Some(stream) => stream,
        None => {
            error!("MITM unable to connect to official server");
            return Ok(());
        }
    };
    loop {
        select! {
            // Read packets coming from the client
            result = Packet::read_async_typed::<Components, TcpStream>(&mut client) => {
                let (component, packet) = result?;
                debug_log_packet(&component, &packet, "From Client");
                packet.write_async(&mut server).await?;
                server.flush().await?;
            }
            // Read packets from the official server
            result = Packet::read_async_typed::<Components, BlazeStream>(&mut server) => {
                let (component, packet) = result?;
                debug_log_packet(&component, &packet, "From Server");
                packet.write_async(&mut client).await?;
            }
        };
    }
}

/// Logs the contents of the provided packet to the debug output along with
/// the header information.
///
/// `component` The component for the packet routing
/// `packet`    The packet that is being logged
/// `direction` The direction name for the packet
fn debug_log_packet(component: &Components, packet: &Packet, direction: &str) {
    let debug = PacketDebug {
        packet,
        component,
        minified: false,
    };
    debug!("\nRecieved Packet {}\n{:?}", direction, debug);
}
