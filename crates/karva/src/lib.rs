//! Top-level CLI entry point shared by the `karva` binary and integration tests.

use std::ffi::OsString;
use std::io;
use std::io::stdout;

use anyhow::Context;
use clap::{CommandFactory, Parser};
use colored::Colorize;
use karva_cli::{Args, Command};

pub use karva_cli::ExitStatus;

mod commands;
mod utils;

pub fn karva_main(f: impl FnOnce(Vec<OsString>) -> Vec<OsString>) -> ExitStatus {
    run(f).unwrap_or_else(|error| {
        use io::Write;

        // Exit "gracefully" on broken pipe errors.
        //
        // See: https://github.com/BurntSushi/ripgrep/blob/bf63fe8f258afc09bae6caa48f0ae35eaf115005/crates/core/main.rs#L47C1-L61C14
        if error.chain().any(|cause| {
            cause
                .downcast_ref::<io::Error>()
                .is_some_and(|ioerr| ioerr.kind() == io::ErrorKind::BrokenPipe)
        }) {
            return ExitStatus::Success;
        }

        // Use `writeln` instead of `eprintln` to avoid panicking when the stderr pipe is broken.
        let mut stderr = io::stderr().lock();

        // This communicates that this isn't a linter error but karva itself hard-errored for
        // some reason (e.g. failed to resolve the configuration)
        writeln!(stderr, "{}", "karva failed".red().bold()).ok();
        // Currently we generally only see one error, but e.g. with io errors when resolving
        // the configuration it is help to chain errors ("resolving configuration failed" ->
        // "failed to read file: subdir/pyproject.toml")
        for cause in error.chain() {
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
        Command::Coverage(coverage_args) => commands::coverage::coverage(&coverage_args),
        Command::Cache(cache_args) => commands::cache::cache(&cache_args),
        Command::ShowConfig(show_config_args) => {
            commands::show_config::show_config(show_config_args)
        }
        Command::Server => commands::server::server(),
        Command::GenerateShellCompletion { shell } => {
            shell.generate(&mut Args::command(), &mut stdout());
            Ok(ExitStatus::Success)
        }
        Command::Version => commands::version::version().map(|()| ExitStatus::Success),
    }
}
