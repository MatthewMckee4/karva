use clap::Parser;

#[derive(Debug, Parser)]
/// Parsed arguments for `karva snapshot`.
pub struct SnapshotCommand {
    /// Snapshot operation to execute.
    #[command(subcommand)]
    pub action: SnapshotAction,
}

#[derive(Debug, clap::Subcommand)]
/// Supported external snapshot operations.
pub enum SnapshotAction {
    /// Accept all (or filtered) pending snapshots.
    Accept(SnapshotFilterArgs),

    /// Reject all (or filtered) pending snapshots.
    Reject(SnapshotFilterArgs),

    /// List pending snapshots.
    Pending(SnapshotFilterArgs),

    /// Interactively review pending snapshots.
    Review(SnapshotFilterArgs),

    /// Remove snapshot files whose source test no longer exists.
    Prune(SnapshotPruneArgs),

    /// Delete all (or filtered) snapshot files (.snap and .snap.new).
    Delete(SnapshotDeleteArgs),
}

#[derive(Debug, Parser, Default)]
/// Path filters shared by non-destructive snapshot operations.
pub struct SnapshotFilterArgs {
    /// Optional paths to filter snapshots by directory or file.
    #[clap(value_name = "PATH")]
    pub paths: Vec<String>,
}

#[derive(Debug, Parser, Default)]
/// Arguments controlling removal of snapshots no longer produced by tests.
pub struct SnapshotPruneArgs {
    /// Optional paths to filter snapshots by directory or file.
    #[clap(value_name = "PATH")]
    pub paths: Vec<String>,

    /// Show which snapshots would be removed without deleting them.
    #[clap(long)]
    pub dry_run: bool,
}

#[derive(Debug, Parser, Default)]
/// Arguments controlling explicit deletion of snapshot files.
pub struct SnapshotDeleteArgs {
    /// Optional paths to filter which snapshot files are deleted.
    #[clap(value_name = "PATH")]
    pub paths: Vec<String>,

    /// Show which snapshot files would be deleted without removing them.
    #[clap(long)]
    pub dry_run: bool,
}
