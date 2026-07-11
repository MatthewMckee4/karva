mod accept;
mod delete;
mod pending;
mod prune;
mod reject;
mod review;

use std::io::Write;

use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use karva_cli::{SnapshotAction, SnapshotCommand};
use karva_logging::{Printer, Stdout};
use karva_project::path::absolute;
use karva_snapshot::storage::{
    PendingSnapshotInfo, find_pending_snapshots, matches_snapshot_filter,
};

use crate::ExitStatus;
use crate::utils::cwd;

pub fn snapshot(args: SnapshotCommand) -> Result<ExitStatus> {
    match args.action {
        SnapshotAction::Accept(filter) => accept::accept(&filter.paths),
        SnapshotAction::Reject(filter) => reject::reject(&filter.paths),
        SnapshotAction::Pending(filter) => pending::pending(&filter.paths),
        SnapshotAction::Review(filter) => review::review(&filter.paths),
        SnapshotAction::Prune(prune_args) => prune::prune(&prune_args.paths, prune_args.dry_run),
        SnapshotAction::Delete(delete_args) => {
            delete::delete(&delete_args.paths, delete_args.dry_run)
        }
    }
}

/// Common setup for snapshot commands: resolves the cwd, creates a printer
/// with locked stdout, and resolves filter paths to absolute paths.
fn snapshot_setup(filter_paths: &[String]) -> Result<(Stdout, Utf8PathBuf, Vec<Utf8PathBuf>)> {
    let cwd = cwd()?;
    let printer = Printer::default();
    let stdout = printer.stream_for_message().lock();
    let resolved = resolve_filter_paths(filter_paths, &cwd);
    Ok((stdout, cwd, resolved))
}

/// Setup for snapshot commands that operate on the set of pending snapshots
/// (`accept`, `reject`, `pending`).
///
/// Returns `Ok(None)` (after writing the empty-state message) when no pending
/// snapshots match the filter, otherwise `Ok(Some((stdout, filtered)))`.
fn pending_setup(filter_paths: &[String]) -> Result<Option<(Stdout, Vec<PendingSnapshotInfo>)>> {
    let (mut stdout, cwd, resolved) = snapshot_setup(filter_paths)?;
    let pending = find_pending_snapshots(&cwd)?;
    let filtered: Vec<_> = pending
        .into_iter()
        .filter(|info| matches_filter(&info.pending_path, &resolved))
        .collect();
    if filtered.is_empty() {
        writeln!(stdout, "No pending snapshots found.")?;
        return Ok(None);
    }
    Ok(Some((stdout, filtered)))
}

/// Filters items by resolved path prefixes and handles the empty case.
///
/// Returns `None` (after writing `empty_message`) when no items match,
/// or `Some(filtered)` with the matching subset.
fn filter_or_empty<'a, T>(
    items: &'a [T],
    resolved: &[Utf8PathBuf],
    path_fn: impl Fn(&T) -> &Utf8Path,
    empty_message: &str,
    stdout: &mut Stdout,
) -> Result<Option<Vec<&'a T>>> {
    let filtered: Vec<_> = items
        .iter()
        .filter(|item| matches_filter(path_fn(item), resolved))
        .collect();
    if filtered.is_empty() {
        writeln!(stdout, "{empty_message}")?;
        return Ok(None);
    }
    Ok(Some(filtered))
}

/// Resolve user-provided filter strings to absolute paths.
fn resolve_filter_paths(filter_paths: &[String], cwd: &Utf8Path) -> Vec<Utf8PathBuf> {
    filter_paths.iter().map(|f| absolute(f, cwd)).collect()
}

/// Check if a snapshot path matches any resolved filter.
///
/// Filters can target either the generated snapshot file or the source file
/// whose stem is encoded in that snapshot filename.
fn matches_filter(snapshot_path: &Utf8Path, resolved_filters: &[Utf8PathBuf]) -> bool {
    resolved_filters.is_empty()
        || resolved_filters
            .iter()
            .any(|filter| matches_snapshot_filter(snapshot_path, filter))
}
