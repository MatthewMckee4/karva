use std::io::Write;

use anyhow::Result;

use super::{filter_or_empty, snapshot_setup};
use crate::ExitStatus;

pub fn delete(filter_paths: &[String], dry_run: bool) -> Result<ExitStatus> {
    let (mut stdout, cwd, resolved) = snapshot_setup(filter_paths)?;
    let all = karva_snapshot::storage::find_all_snapshots(&cwd)?;
    let Some(filtered) = filter_or_empty(
        &all,
        &resolved,
        |i| &i.path,
        "No snapshot files found.",
        &mut stdout,
    )?
    else {
        return Ok(ExitStatus::Success);
    };
    if dry_run {
        for info in &filtered {
            writeln!(stdout, "Would delete: {}", info.path)?;
        }
        writeln!(
            stdout,
            "\n{} snapshot file(s) would be deleted.",
            filtered.len()
        )?;
    } else {
        for info in &filtered {
            karva_snapshot::storage::remove_snapshot(&info.path)?;
            writeln!(stdout, "Deleted: {}", info.path)?;
        }
        writeln!(stdout, "\n{} snapshot file(s) deleted.", filtered.len())?;
    }
    Ok(ExitStatus::Success)
}
