use std::sync::Arc;

use log::info;
use tokio::signal;

mod components;
mod logging;
mod mitm;
mod models;
mod redirector;
mod retriever;

/// The external address of the server. This address is whats used in
/// the system hosts file as a redirect so theres no need to use any
/// other address.
pub const EXTERNAL_HOST: &str = "gosredirector.ea.com";
/// The server version extracted from the Cargo.toml
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const REDIRECTOR_PORT: u16 = 42127;
pub const MAIN_PORT: u16 = 42128;

fn main() {
    // Create the tokio runtime
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed building the tokio Runtime");

    logging::setup();

    info!("Starting Pocket Relay MITM v{}", VERSION);

    let retriever = runtime
        .block_on(retriever::Retriever::new())
        .expect("Failed to initialize connection to official server");

    // Spawn redirector in its own task
    runtime.spawn(redirector::start_server());

    // Start the MITM server
    runtime.spawn(mitm::start_server(Arc::new(retriever)));

    // Block until shutdown is recieved
    runtime.block_on(signal::ctrl_c()).ok();

    info!("Shutting down...");
}
