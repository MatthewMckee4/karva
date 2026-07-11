use std::io::{self, BufRead, Write};

use camino::{Utf8Path, Utf8PathBuf};
use colored::Colorize;
use console::{Key, Term};

use crate::diff::print_changeset;
use crate::storage::{
    PendingSnapshotInfo, accept_pending, accept_pending_batch, find_pending_snapshots,
    matches_snapshot_filter, read_snapshot, reject_pending,
};

/// Result of reviewing all pending snapshots.
#[derive(Debug, Default)]
pub struct ReviewSummary {
    pub accepted: Vec<String>,
    pub rejected: Vec<String>,
    pub skipped: Vec<String>,
}

/// Action chosen by the user for a single snapshot.
enum ReviewAction {
    Accept,
    Reject,
    Skip,
    AcceptAll,
    RejectAll,
    SkipAll,
    ToggleInfo,
    ToggleDiff,
}

/// Write the insta-style action menu to the output.
fn write_prompt(out: &mut impl Write, show_info: bool, show_diff: bool) -> io::Result<()> {
    let info_label = if show_info { "hide info" } else { "show info" };
    let diff_label = if show_diff { "hide diff" } else { "show diff" };

    writeln!(out)?;
    writeln!(
        out,
        "  {} {:<11}keep the new snapshot",
        "a".green(),
        "accept"
    )?;
    writeln!(
        out,
        "  {} {:<11}retain the old snapshot",
        "r".red(),
        "reject"
    )?;
    writeln!(out, "  {} {:<11}keep both for now", "s".yellow(), "skip")?;
    writeln!(
        out,
        "  {} {info_label:<11}toggles extended snapshot info",
        "i".blue()
    )?;
    writeln!(out, "  {} {diff_label:<11}toggle snapshot diff", "d".blue())?;
    writeln!(out)?;
    writeln!(
        out,
        "  Tip: Use uppercase A/R/S to apply to all remaining snapshots"
    )?;
    Ok(())
}

/// Read the user's review action.
///
/// Uses single-keypress input when running in a terminal, or falls back to
/// line-buffered stdin when piped (e.g., in tests).
fn read_review_action(out: &mut impl Write) -> io::Result<ReviewAction> {
    let term = Term::stdout();

    if term.is_term() {
        let key = term.read_key()?;
        match key {
            Key::Char('a') | Key::Enter => Ok(ReviewAction::Accept),
            Key::Char('r') | Key::Escape => Ok(ReviewAction::Reject),
            Key::Char('s' | ' ') => Ok(ReviewAction::Skip),
            Key::Char('A') => Ok(ReviewAction::AcceptAll),
            Key::Char('R') => Ok(ReviewAction::RejectAll),
            Key::Char('S') => Ok(ReviewAction::SkipAll),
            Key::Char('i') => Ok(ReviewAction::ToggleInfo),
            Key::Char('d') => Ok(ReviewAction::ToggleDiff),
            _ => Ok(ReviewAction::Skip),
        }
    } else {
        write!(out, "> ")?;
        out.flush()?;

        let stdin = io::stdin();
        let mut input = String::new();
        stdin.lock().read_line(&mut input)?;

        match input.trim() {
            "a" => Ok(ReviewAction::Accept),
            "r" => Ok(ReviewAction::Reject),
            "s" | "" => Ok(ReviewAction::Skip),
            "A" => Ok(ReviewAction::AcceptAll),
            "R" => Ok(ReviewAction::RejectAll),
            "S" => Ok(ReviewAction::SkipAll),
            _ => Ok(ReviewAction::Skip),
        }
    }
}

/// Run an interactive review session for all pending snapshots under the given root.
///
/// For each pending snapshot, displays the diff and prompts the user for an action.
pub fn run_review(root: &Utf8Path, resolved_filters: &[Utf8PathBuf]) -> io::Result<ReviewSummary> {
    let pending = find_pending_snapshots(root)?;

    let filtered: Vec<_> = if resolved_filters.is_empty() {
        pending
    } else {
        pending
            .into_iter()
            .filter(|info| {
                resolved_filters
                    .iter()
                    .any(|filter| matches_snapshot_filter(&info.pending_path, filter))
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
    let stdout = io::stdout();
    let mut show_info = true;
    let mut show_diff = true;

    'outer: for (i, info) in filtered.iter().enumerate() {
        loop {
            let mut out = stdout.lock();

            writeln!(out)?;
            writeln!(out, "Snapshot {}/{total}", i + 1)?;

            if show_info {
                writeln!(out, "File: {}", info.pending_path)?;
                if let Some(source) =
                    read_snapshot(&info.pending_path)?.and_then(|s| s.metadata.source)
                {
                    writeln!(out, "Source: {source}")?;
                }
            }

            if show_diff {
                print_snapshot_diff(&mut out, info)?;
            }

            write_prompt(&mut out, show_info, show_diff)?;
            out.flush()?;

            let action = read_review_action(&mut out)?;

            match action {
                ReviewAction::ToggleInfo => {
                    show_info = !show_info;
                }
                ReviewAction::ToggleDiff => {
                    show_diff = !show_diff;
                }
                ReviewAction::Accept => {
                    accept_pending(&info.pending_path)?;
                    summary.accepted.push(info.pending_path.to_string());
                    break;
                }
                ReviewAction::Reject => {
                    reject_pending(&info.pending_path)?;
                    summary.rejected.push(info.pending_path.to_string());
                    break;
                }
                ReviewAction::Skip => {
                    summary.skipped.push(info.pending_path.to_string());
                    break;
                }
                ReviewAction::AcceptAll => {
                    accept_pending_batch(&filtered[i..])?;
                    for item in &filtered[i..] {
                        summary.accepted.push(item.pending_path.to_string());
                    }
                    break 'outer;
                }
                ReviewAction::RejectAll => {
                    reject_pending(&info.pending_path)?;
                    summary.rejected.push(info.pending_path.to_string());
                    for remaining in &filtered[i + 1..] {
                        reject_pending(&remaining.pending_path)?;
                        summary.rejected.push(remaining.pending_path.to_string());
                    }
                    break 'outer;
                }
                ReviewAction::SkipAll => {
                    summary.skipped.push(info.pending_path.to_string());
                    for remaining in &filtered[i + 1..] {
                        summary.skipped.push(remaining.pending_path.to_string());
                    }
                    break 'outer;
                }
            }
        }
    }

    let mut out = stdout.lock();
    writeln!(out)?;
    writeln!(out, "review finished")?;

    print_summary_section(&mut out, "accepted:", &summary.accepted)?;
    print_summary_section(&mut out, "rejected:", &summary.rejected)?;
    print_summary_section(&mut out, "skipped:", &summary.skipped)?;

    Ok(summary)
}

/// Print a labeled list of paths if non-empty.
fn print_summary_section(out: &mut impl Write, label: &str, paths: &[String]) -> io::Result<()> {
    if !paths.is_empty() {
        writeln!(out, "{label}")?;
        for path in paths {
            writeln!(out, "  {path}")?;
        }
    }
    Ok(())
}

fn print_snapshot_diff(out: &mut impl Write, info: &PendingSnapshotInfo) -> io::Result<()> {
    let old_content = read_snapshot(&info.snap_path)?
        .map(|s| s.content)
        .unwrap_or_default();

    let new_content = read_required_snapshot_content(&info.pending_path)?;

    writeln!(out)?;
    print_changeset(out, &old_content, &new_content)?;

    Ok(())
}

fn read_required_snapshot_content(path: &Utf8Path) -> io::Result<String> {
    read_snapshot(path)?
        .map(|snapshot| snapshot.content)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("Pending snapshot `{path}` no longer exists"),
            )
        })
}

#[cfg(test)]
mod tests {
    use camino::Utf8Path;

    use super::read_required_snapshot_content;

    #[test]
    fn required_snapshot_content_reports_missing_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dir_path = Utf8Path::from_path(dir.path()).expect("utf8");
        let missing = dir_path.join("snapshots/test__test_foo.snap.new");

        let err = read_required_snapshot_content(&missing).expect_err("missing pending snapshot");
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }
}
