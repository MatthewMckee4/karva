use std::io::Write;

use anyhow::Result;

use super::pending_setup;
use crate::ExitStatus;

pub fn pending(filter_paths: &[String]) -> Result<ExitStatus> {
    let Some((mut stdout, filtered)) = pending_setup(filter_paths)? else {
        return Ok(ExitStatus::Success);
    };
    for info in &filtered {
        writeln!(stdout, "{}", info.pending_path)?;
    }
    writeln!(stdout, "\n{} pending snapshot(s).", filtered.len())?;
    Ok(ExitStatus::Success)
}
