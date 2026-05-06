use std::ffi::OsString;
use std::io;
use std::process::{ExitCode, Termination};

use anyhow::Context;
use clap::Parser;
use colored::Colorize;
use karva_cli::{Args, Command};

mod commands;
mod utils;

pub fn karva_main(f: impl FnOnce(Vec<OsString>) -> Vec<OsString>) -> ExitStatus {
    run(f).unwrap_or_else(|error| {
        use std::io::Write;

        let mut stderr = std::io::stderr().lock();

        writeln!(stderr, "{}", "Karva failed".red().bold()).ok();
        for cause in error.chain() {
            if let Some(ioerr) = cause.downcast_ref::<io::Error>() {
                if ioerr.kind() == io::ErrorKind::BrokenPipe {
                    return ExitStatus::Success;
                }
            }

            writeln!(stderr, "  {} {cause}", "Cause:".bold()).ok();
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

#[derive(Copy, Clone)]
pub enum ExitStatus {
    /// Checking was successful and there were no errors.
    Success = 0,

    /// Checking was successful but there were errors.
    Failure = 1,

    /// Checking failed.
    Error = 2,
}

impl Termination for ExitStatus {
    fn report(self) -> ExitCode {
        ExitCode::from(self as u8)
    }
}

impl ExitStatus {
    pub fn to_i32(self) -> i32 {
        self as i32
    }
}
