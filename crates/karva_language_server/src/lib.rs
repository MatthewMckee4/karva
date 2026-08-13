//! Language Server Protocol support for Karva.

use anyhow::Context;
use std::io::{self, Write as _};

pub use server::{ConnectionInitializer, Server};

mod capabilities;
mod document;
mod server;
mod session;
mod workspace;

pub use document::{PositionEncoding, TextDocument};

const SERVER_NAME: &str = "karva";

/// Runs the Karva language server over standard input and output.
pub fn run() -> anyhow::Result<()> {
    match std::env::args().nth(1).as_deref() {
        None => run_server(),
        Some("--version" | "-V") => {
            write_stdout(&format!("{SERVER_NAME} {}", karva_version::version()))
        }
        Some("--help" | "-h") => write_stdout(
            "Karva language server\n\nUsage: karva-language-server [OPTIONS]\n\nOptions:\n  -V, --version  Print the server version\n  -h, --help     Print this help message",
        ),
        Some(argument) => anyhow::bail!(
            "unexpected argument `{argument}`; the language server communicates over stdio"
        ),
    }
}

fn write_stdout(output: &str) -> anyhow::Result<()> {
    writeln!(io::stdout().lock(), "{output}").context("failed to write command output")
}

fn run_server() -> anyhow::Result<()> {
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
