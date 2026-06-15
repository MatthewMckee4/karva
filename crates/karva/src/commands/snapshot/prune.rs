use std::io::Write;

use anyhow::Result;
use colored::Colorize;

use super::{filter_or_empty, snapshot_setup};
use crate::ExitStatus;

pub fn prune(filter_paths: &[String], dry_run: bool) -> Result<ExitStatus> {
    {
        writeln!(
            std::io::stderr(),
            "{} Prune uses static analysis and may not detect all unreferenced snapshots.",
            "warning:".yellow().bold()
        )?;
    }
    let (mut stdout, cwd, resolved) = snapshot_setup(filter_paths)?;
    let unreferenced = karva_snapshot::storage::find_unreferenced_snapshots(&cwd)?;
    let Some(filtered) = filter_or_empty(
        &unreferenced,
        &resolved,
        |i| &i.snap_path,
        "No unreferenced snapshots found.",
        &mut stdout,
    )?
    else {
        return Ok(ExitStatus::Success);
    };
    if dry_run {
        for info in &filtered {
            writeln!(stdout, "Would remove: {} ({})", info.snap_path, info.reason)?;
        }
        writeln!(
            stdout,
            "\n{} unreferenced snapshot(s) would be removed.",
            filtered.len()
        )?;
    } else {
        for info in &filtered {
            karva_snapshot::storage::remove_snapshot(&info.snap_path)?;
            writeln!(stdout, "Removed: {} ({})", info.snap_path, info.reason)?;
        }
        writeln!(stdout, "\n{} snapshot(s) pruned.", filtered.len())?;
    }
    Ok(ExitStatus::Success)
}
