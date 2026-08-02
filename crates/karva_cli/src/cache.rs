use clap::Parser;

#[derive(Debug, Parser)]
/// Parsed arguments for `karva cache`.
pub struct CacheCommand {
    /// Cache maintenance operation to execute.
    #[command(subcommand)]
    pub action: CacheAction,
}

#[derive(Debug, clap::Subcommand)]
/// Supported cache maintenance operations.
pub enum CacheAction {
    /// Remove all but the most recent test run from the cache.
    Prune,

    /// Remove the entire cache directory.
    Clean,
}
