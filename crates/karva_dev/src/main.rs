//! This crate implements an internal CLI for developers of Karva.
//!
//! Within the Karva repository you can run it with `cargo dev`.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};

mod generate_cli_reference;

const ROOT_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../");

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate CLI reference.
    GenerateCliReference(generate_cli_reference::Args),
}

fn main() -> Result<ExitCode> {
    let Args { command } = Args::parse();
    match command {
        Command::GenerateCliReference(args) => generate_cli_reference::main(&args)?,
    }
    Ok(ExitCode::SUCCESS)
}
