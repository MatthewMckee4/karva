use std::io::Write;

use anyhow::Result;
use camino::Utf8Path;

use crate::ExitStatus;

pub fn prune(cwd: &Utf8Path, stdout: &mut impl Write) -> Result<ExitStatus> {
    let cache_dir = cwd.join(karva_cache::CACHE_DIR);
    let result = karva_cache::prune_cache(&cache_dir)?;
    for dir_name in &result.removed {
        writeln!(stdout, "Removed: {dir_name}")?;
    }
    if result.removed.is_empty() {
        writeln!(stdout, "No cache runs to prune.")?;
    } else {
        writeln!(stdout, "\n{} run(s) pruned.", result.removed.len())?;
    }
    Ok(ExitStatus::Success)
}
