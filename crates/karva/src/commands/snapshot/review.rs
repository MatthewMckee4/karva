use anyhow::Result;

use super::resolve_filter_paths;
use crate::ExitStatus;
use crate::utils::cwd;

pub fn review(filter_paths: &[String]) -> Result<ExitStatus> {
    let cwd = cwd()?;
    let resolved = resolve_filter_paths(filter_paths, &cwd);
    karva_snapshot_review::run_review(&cwd, &resolved)?;
    Ok(ExitStatus::Success)
}
