//! Module for the Redirector server which handles redirecting the clients
//! to the correct address for the main server.

use crate::{
    components::{Components, Redirector},
    models::{InstanceDetails, InstanceNet},
    EXTERNAL_HOST, MAIN_PORT, REDIRECTOR_PORT,
};
use blaze_pk::packet::Packet;
use blaze_ssl_async::{BlazeAccept, BlazeListener};
use log::{debug, error, info};
use std::io;
use tokio::{io::AsyncWriteExt, select};

/// Starts the Redirector server this server is what the Mass Effect 3 game
/// client initially reaches out to. This server is responsible for telling
/// the client where the server is and whether it should use SSLv3 to connect.
pub async fn start_server() {
    // Initializing the underlying TCP listener
    let listener = {
        let port = REDIRECTOR_PORT;
        match BlazeListener::bind(("0.0.0.0", port)).await {
            Ok(value) => {
                info!("Started Redirector server (Port: {})", port);
                value
            }
            Err(_) => {
                error!("Failed to bind Redirector server (Port: {})", port);
                panic!()
            }
        }
    };

    // Accept incoming connections
    loop {
        let accept = match listener.accept().await {
            Ok(value) => value,
            Err(err) => {
                error!("Failed to accept redirector connection: {err:?}");
                continue;
            }
        };
        tokio::spawn(async move {
            if let Err(err) = handle_client(accept).await {
                error!("Unable to handle redirect: {err}");
            };
        });
    }
}

/// The component to look for when waiting for redirects
const REDIRECT_COMPONENT: Components = Components::Redirector(Redirector::GetServerInstance);

/// Handles dealing with a redirector client
///
/// `stream`   The stream to the client
/// `addr`     The client address
/// `instance` The server instance information
async fn handle_client(accept: BlazeAccept) -> io::Result<()> {
    let (mut stream, addr) = match accept.finish_accept().await {
        Ok(value) => value,
        Err(err) => {
            error!(
                "Unable to establish SSL connection within redirector: {:?}",
                err
            );
            return Ok(());
        }
    };

    loop {
        let (component, packet): (Components, Packet) = select! {
            // Attempt to read packets from the stream
            result = Packet::read_async_typed(&mut stream) => result,
        }?;

        if component == REDIRECT_COMPONENT {
            debug!("Redirecting client (Addr: {addr:?})");

            let host = EXTERNAL_HOST;
            let port = MAIN_PORT;
            let instance = InstanceDetails {
                net: InstanceNet::from((host.to_string(), port)),
                secure: false,
            };

            let response = Packet::response(&packet, instance);
            response.write_async(&mut stream).await?;
            stream.flush().await?;
            break;
        } else {
            let response = Packet::response_empty(&packet);
            response.write_async(&mut stream).await?;
            stream.flush().await?;
        }
    }

    Ok(())
}
