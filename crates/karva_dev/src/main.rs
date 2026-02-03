//! This crate implements an internal CLI for developers of Karva.
//!
//! Within the Karva repository you can run it with `cargo run -p karva_dev`.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};

mod generate_cli_reference;
mod generate_options;

const ROOT_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../");

pub const REGENERATE_ALL_COMMAND: &str = "cargo run -p karva_dev generate-all";

#[derive(Copy, Clone, PartialEq, Eq, clap::ValueEnum, Default)]
pub(crate) enum Mode {
    /// Update the content in the `configuration.md`.
    #[default]
    Write,

    /// Don't write to the file, check if the file is up-to-date and error if not.
    Check,

    /// Write the generated help to stdout.
    DryRun,
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
#[expect(clippy::enum_variant_names)]
enum Command {
    /// Generate CLI reference.
    GenerateCliReference(generate_cli_reference::Args),
    /// Generate options reference.
    GenerateOptions(generate_options::Args),
    /// Generate all developer documentation and references.
    GenerateAll,
}

fn main() -> Result<ExitCode> {
    let Args { command } = Args::parse();
    match command {
        Command::GenerateCliReference(args) => generate_cli_reference::main(&args)?,
        Command::GenerateAll => {
            generate_cli_reference::main(&generate_cli_reference::Args { mode: Mode::Write })?;
            generate_options::main(&generate_options::Args { mode: Mode::Write })?;
        }
        Command::GenerateOptions(args) => generate_options::main(&args)?,
    }
    Ok(ExitCode::SUCCESS)
}
