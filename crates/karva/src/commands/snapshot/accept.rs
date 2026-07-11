use std::io::Write;

use anyhow::Result;

use super::pending_setup;
use crate::ExitStatus;

pub fn accept(filter_paths: &[String]) -> Result<ExitStatus> {
    let Some((mut stdout, filtered)) = pending_setup(filter_paths)? else {
        return Ok(ExitStatus::Success);
    };
    karva_snapshot::storage::accept_pending_batch(&filtered)?;
    for info in &filtered {
        writeln!(stdout, "Accepted: {}", info.pending_path)?;
    }
    writeln!(stdout, "\n{} snapshot(s) accepted.", filtered.len())?;
    Ok(ExitStatus::Success)
}
