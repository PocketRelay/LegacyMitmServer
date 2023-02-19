//! Module for the Redirector server which handles redirecting the clients
//! to the correct address for the main server.

use blaze_pk::packet::{Packet, PacketDebug};
use log::{debug, error, info};
use std::sync::Arc;
use tokio::{
    io::{split, AsyncRead, AsyncWrite, AsyncWriteExt, ReadHalf},
    net::TcpListener,
    sync::mpsc,
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
        let (stream, _) = match listener.accept().await {
            Ok(value) => value,
            Err(err) => {
                error!("Failed to accept MITM connection: {err:?}");
                continue;
            }
        };
        let retriever = retriever.clone();
        tokio::spawn(async move {
            let server = match retriever.stream().await {
                Some(stream) => stream,
                None => {
                    error!("MITM unable to connect to official server");
                    return;
                }
            };

            let (client_reader, client_writer) = split(stream);
            let client_writer = Writer::start(client_writer);

            let (server_reader, server_writer) = split(server);
            let server_writer = Writer::start(server_writer);

            Reader::spawn(client_reader, server_writer, "Client");
            Reader::spawn(server_reader, client_writer, "Server");
        });
    }
}

/// Writer for writing packets to a connection
struct Writer<W> {
    rx: mpsc::UnboundedReceiver<Packet>,
    write: W,
}

impl<W> Writer<W>
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    pub fn start(write: W) -> WriterAddr {
        let (tx, rx) = mpsc::unbounded_channel();
        let writer = Writer { rx, write };
        tokio::spawn(writer.process());
        WriterAddr(tx)
    }

    pub async fn process(mut self) {
        while let Some(packet) = self.rx.recv().await {
            if let Err(err) = packet.write_async(&mut self.write).await {
                error!("Error while write: {:?}", err)
            }
            if let Err(err) = self.write.flush().await {
                error!("Error while flushing: {:?}", err);
            }
        }
    }
}

#[derive(Clone)]
struct WriterAddr(mpsc::UnboundedSender<Packet>);

struct Reader<R> {
    /// Reader to read the packets from
    read: R,
    /// Writer to send the packets to
    writer: WriterAddr,

    side: &'static str,
}

impl<R> Reader<R>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    pub fn spawn(read: R, writer: WriterAddr, side: &'static str) {
        let reader = Reader { read, writer, side };
        tokio::spawn(reader.process());
    }

    pub async fn process(mut self) {
        loop {
            let (component, packet) =
                match Packet::read_async_typed::<Components, R>(&mut self.read).await {
                    Ok(value) => value,
                    Err(err) => {
                        error!("Error while reading: {:?}", err);
                        break;
                    }
                };
            debug_log_packet(&component, &packet, self.side);
            self.writer.0.send(packet).ok();
        }
    }
}

/// Logs the contents of the provided packet to the debug output along with
/// the header information.
///
/// `component` The component for the packet routing
/// `packet`    The packet that is being logged
/// `direction` The direction name for the packet
fn debug_log_packet(component: &Components, packet: &Packet, side: &str) {
    let debug = PacketDebug {
        packet,
        component,
        minified: false,
    };
    debug!("\nRecieved Packet From {}\n{:?}", side, debug);
}
