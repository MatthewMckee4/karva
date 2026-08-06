pub(crate) mod common;

mod r#async;
mod basic;
mod cache;
mod cancel;
mod configuration;
mod coverage;
mod coverage_command;
#[cfg(unix)]
mod diagnostic_svg;
mod discovery;
mod doctest;
mod durations;
mod extensions;
mod filterset;
mod junit;
mod last_failed;
mod partition;
mod result_report;
mod run_ignored;
mod run_timeout;
mod server;
mod show_config;
mod shuffle;
mod version;
mod watch;
mod worker_crash;
