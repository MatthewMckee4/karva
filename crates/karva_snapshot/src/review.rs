use std::io::{self, BufRead, Write};

use camino::Utf8Path;

use crate::diff::format_diff;
use crate::storage::{
    PendingSnapshotInfo, accept_pending, find_pending_snapshots, read_snapshot, reject_pending,
};

/// Result of reviewing all pending snapshots.
#[derive(Debug, Default)]
pub struct ReviewSummary {
    pub accepted: usize,
    pub rejected: usize,
    pub skipped: usize,
}

/// Run an interactive review session for all pending snapshots under the given root.
///
/// For each pending snapshot, displays the diff and prompts the user for an action.
pub fn run_review(root: &Utf8Path, filter_paths: &[String]) -> io::Result<ReviewSummary> {
    let pending = find_pending_snapshots(root);

    let filtered: Vec<_> = if filter_paths.is_empty() {
        pending
    } else {
        pending
            .into_iter()
            .filter(|info| {
                filter_paths
                    .iter()
                    .any(|f| info.pending_path.as_str().contains(f.as_str()))
            })
            .collect()
    };

    if filtered.is_empty() {
        let stdout = io::stdout();
        let mut out = stdout.lock();
        writeln!(out, "No pending snapshots to review.")?;
        return Ok(ReviewSummary::default());
    }

    let total = filtered.len();
    let mut summary = ReviewSummary::default();
    let stdin = io::stdin();
    let stdout = io::stdout();

    for (i, info) in filtered.iter().enumerate() {
        let mut out = stdout.lock();

        writeln!(out)?;
        writeln!(out, "Snapshot {}/{total}", i + 1)?;
        writeln!(out, "File: {}", info.pending_path)?;

        if let Some(source) = read_snapshot(&info.pending_path).and_then(|s| s.metadata.source) {
            writeln!(out, "Source: {source}")?;
        }

        print_snapshot_diff(&mut out, info)?;

        writeln!(
            out,
            "\n(a)ccept  (r)eject  (s)kip  (A)ccept all  (R)eject all"
        )?;
        write!(out, "> ")?;
        out.flush()?;

        let mut input = String::new();
        stdin.lock().read_line(&mut input)?;

        match input.trim() {
            "a" => {
                accept_pending(&info.pending_path)?;
                summary.accepted += 1;
            }
            "r" => {
                reject_pending(&info.pending_path)?;
                summary.rejected += 1;
            }
            "s" | "" => {
                summary.skipped += 1;
            }
            "A" => {
                accept_pending(&info.pending_path)?;
                summary.accepted += 1;
                for remaining in &filtered[i + 1..] {
                    accept_pending(&remaining.pending_path)?;
                    summary.accepted += 1;
                }
                break;
            }
            "R" => {
                reject_pending(&info.pending_path)?;
                summary.rejected += 1;
                for remaining in &filtered[i + 1..] {
                    reject_pending(&remaining.pending_path)?;
                    summary.rejected += 1;
                }
                break;
            }
            _ => {
                summary.skipped += 1;
            }
        }
    }

    let mut out = stdout.lock();
    writeln!(out)?;
    writeln!(
        out,
        "Review complete: {} accepted, {} rejected, {} skipped",
        summary.accepted, summary.rejected, summary.skipped
    )?;

    Ok(summary)
}

fn print_snapshot_diff(out: &mut impl Write, info: &PendingSnapshotInfo) -> io::Result<()> {
    let old_content = read_snapshot(&info.snap_path)
        .map(|s| s.content)
        .unwrap_or_default();

    let new_content = read_snapshot(&info.pending_path)
        .map(|s| s.content)
        .unwrap_or_default();

    writeln!(out)?;
    write!(out, "{}", format_diff(&old_content, &new_content))?;

    Ok(())
}
