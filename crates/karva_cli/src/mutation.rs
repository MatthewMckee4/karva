//! Mutation commands and shared candidate filters.

use std::num::NonZeroUsize;

use camino::Utf8PathBuf;
use clap::{Args, Parser, Subcommand};

/// Generate, run, and inspect Python mutations.
#[derive(Debug, Parser)]
pub struct MutationCommand {
    /// Explicit configuration file, replacing automatic discovery.
    #[arg(long, global = true)]
    pub config_file: Option<Utf8PathBuf>,

    /// Configuration profile to resolve.
    #[arg(long, global = true)]
    pub profile: Option<String>,

    /// Emit native, versioned JSON instead of terminal text.
    #[arg(long, global = true)]
    pub json: bool,

    /// Mutation operation to perform.
    #[command(subcommand)]
    pub action: MutationAction,
}

/// Operations on generated candidates and durable results.
#[derive(Debug, Subcommand)]
pub enum MutationAction {
    /// Verify the baseline and execute mutations, reusing valid results.
    Run(MutationRunArgs),

    /// Report stored results without running tests.
    Results(MutationFilters),

    /// Show a candidate's exact diff, outcome, duration, and executed tests.
    Show {
        /// Full mutant ID or unique ID prefix.
        mutant: String,
    },

    /// Rerun recorded candidates after changing tests.
    Retest(MutationRetestArgs),

    /// Apply one source-bound candidate, refusing stale or conflicting source.
    Apply {
        /// Full mutant ID or unique ID prefix.
        mutant: String,
    },
}

/// Filters shared by execution, retesting, and reporting.
#[derive(Debug, Default, Args)]
pub struct MutationFilters {
    /// Project-relative path glob; repeat to include more paths.
    #[arg(long)]
    pub path: Vec<String>,

    /// Qualified symbol glob; repeat to include more symbols.
    #[arg(long)]
    pub symbol: Vec<String>,

    /// Mutation family: literal, comparison, boolean, arithmetic, control-flow, return.
    #[arg(long)]
    pub operator: Vec<String>,

    /// Prior outcome: pending, killed, survived, uncovered, timed-out, invalid, errored.
    #[arg(long)]
    pub outcome: Vec<String>,
}

/// Inputs controlling one mutation run.
#[derive(Debug, Default, Args)]
pub struct MutationRunArgs {
    /// Files or directories containing application source, relative to project root.
    #[arg(long)]
    pub source: Vec<String>,

    /// Project-relative exclusion glob; repeat to exclude more paths.
    #[arg(long)]
    pub exclude: Vec<String>,

    /// Maximum simultaneously running mutant workers.
    #[arg(short = 'n', long)]
    pub workers: Option<NonZeroUsize>,

    /// Override the measured per-mutant budget, in seconds; must be positive and finite.
    #[arg(long)]
    pub timeout: Option<f64>,

    /// Run every test for every candidate, bypassing affected-test ordering.
    #[arg(long)]
    pub full_suite: bool,

    /// Rerun candidates even when prior results remain valid.
    #[arg(long)]
    pub no_cache: bool,

    /// Candidate filters applied before execution.
    #[command(flatten)]
    pub filters: MutationFilters,
}

/// Selection of recorded candidates to rerun.
#[derive(Debug, Default, Args)]
pub struct MutationRetestArgs {
    /// Optional full mutant ID or unique ID prefix; omit to retest the filtered set.
    pub mutant: Option<String>,

    /// Execution settings and candidate filters.
    #[command(flatten)]
    pub run: MutationRunArgs,
}
