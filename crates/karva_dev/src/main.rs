//! This crate implements an internal CLI for developers of Karva.
//!
//! Within the Karva repository you can run it with `cargo run -p karva_dev`.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::process::ExitCode;

use anyhow::{Result, bail};
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use fs_err as fs;
use pretty_assertions::StrComparison;

mod generate_cli_reference;
mod generate_env_vars;
mod generate_options;

const ROOT_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../");

const REGENERATE_ALL_COMMAND: &str = "cargo run -p karva_dev generate-all";

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

/// Apply `mode` against the file at `relative_path` (relative to `ROOT_DIR`),
/// using `generated` as the desired contents.
pub(crate) fn apply_mode(mode: Mode, relative_path: &str, generated: &str) -> Result<()> {
    let path = Utf8PathBuf::from(ROOT_DIR).join(relative_path);

    match mode {
        Mode::DryRun => {
            println!("{generated}");
        }
        Mode::Check => match fs::read_to_string(&path) {
            Ok(current) if current == generated => {
                println!("Up-to-date: {relative_path}");
            }
            Ok(current) => {
                let comparison = StrComparison::new(&current, generated);
                bail!(
                    "{relative_path} changed, please run `{REGENERATE_ALL_COMMAND}`:\n{comparison}"
                );
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                bail!("{relative_path} not found, please run `{REGENERATE_ALL_COMMAND}`");
            }
            Err(err) => {
                bail!("{relative_path} changed, please run `{REGENERATE_ALL_COMMAND}`:\n{err}");
            }
        },
        Mode::Write => match fs::read_to_string(&path) {
            Ok(current) if current == generated => {
                println!("Up-to-date: {relative_path}");
            }
            Ok(_) | Err(_) => {
                println!("Updating: {relative_path}");
                fs::write(&path, generated.as_bytes())?;
            }
        },
    }

    Ok(())
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
    /// Generate environment variables reference.
    GenerateEnvVars(generate_env_vars::Args),
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
            generate_env_vars::main(&generate_env_vars::Args { mode: Mode::Write })?;
            generate_options::main(&generate_options::Args { mode: Mode::Write })?;
        }
        Command::GenerateEnvVars(args) => generate_env_vars::main(&args)?,
        Command::GenerateOptions(args) => generate_options::main(&args)?,
    }
    Ok(ExitCode::SUCCESS)
}
