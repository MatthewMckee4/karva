use std::io::Write;

use anyhow::Result;

use super::pending_setup;
use crate::ExitStatus;

pub fn reject(filter_paths: &[String]) -> Result<ExitStatus> {
    let Some((mut stdout, filtered)) = pending_setup(filter_paths)? else {
        return Ok(ExitStatus::Success);
    };
    for info in &filtered {
        karva_snapshot::storage::reject_pending(&info.pending_path)?;
        writeln!(stdout, "Rejected: {}", info.pending_path)?;
    }
    writeln!(stdout, "\n{} snapshot(s) rejected.", filtered.len())?;
    Ok(ExitStatus::Success)
}
