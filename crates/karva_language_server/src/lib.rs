//! Language Server Protocol support for Karva.

use anyhow::Context;

pub use server::{ConnectionInitializer, Server};

mod capabilities;
mod server;
mod session;

const SERVER_NAME: &str = "karva";

/// Runs the Karva language server over standard input and output.
pub fn run() -> anyhow::Result<()> {
    let (connection, io_threads) = ConnectionInitializer::stdio();
    let server_result = Server::new(connection)
        .context("failed to initialize language server")?
        .run();
    let io_result = io_threads.join();

    match (server_result, io_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(server), Err(io)) => Err(server).context(format!("I/O thread error: {io}")),
        (Err(server), _) => Err(server),
        (_, Err(io)) => Err(io).context("I/O thread error"),
    }
}
