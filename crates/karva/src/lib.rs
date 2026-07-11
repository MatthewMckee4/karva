use std::ffi::OsString;

use anyhow::Context;
use clap::Parser;
use karva_cli::{Args, Command};

pub use karva_cli::ExitStatus;

mod commands;
mod utils;

pub fn karva_main(f: impl FnOnce(Vec<OsString>) -> Vec<OsString>) -> ExitStatus {
    run(f).unwrap_or_else(|error| {
        if karva_logging::error_chain_contains_broken_pipe(error.chain()) {
            return ExitStatus::Success;
        }

        let mut stderr = std::io::stderr().lock();
        if let Err(err) = karva_logging::write_error_chain(&mut stderr, error.chain()) {
            if err.kind() == std::io::ErrorKind::BrokenPipe {
                return ExitStatus::Success;
            }
        }
        ExitStatus::Error
    })
}

fn run(f: impl FnOnce(Vec<OsString>) -> Vec<OsString>) -> anyhow::Result<ExitStatus> {
    let args = wild::args_os();

    let args = f(
        argfile::expand_args_from(args, argfile::parse_fromfile, argfile::PREFIX)
            .context("Failed to read CLI arguments from file")?,
    );

    let args = Args::parse_from(args);

    match args.command {
        Command::Test(test_args) => commands::test::test(*test_args),
        Command::Snapshot(snapshot_args) => commands::snapshot::snapshot(snapshot_args),
        Command::Cache(cache_args) => commands::cache::cache(&cache_args),
        Command::ShowConfig(show_config_args) => {
            commands::show_config::show_config(show_config_args)
        }
        Command::Version => commands::version::version().map(|()| ExitStatus::Success),
    }
}
