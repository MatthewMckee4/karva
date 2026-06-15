use std::io::Write;

use anyhow::Result;
use camino::Utf8Path;

use crate::ExitStatus;

pub fn clean(cwd: &Utf8Path, stdout: &mut impl Write) -> Result<ExitStatus> {
    let cache_dir = cwd.join(karva_cache::CACHE_DIR);
    if karva_cache::clean_cache(&cache_dir)? {
        writeln!(stdout, "Cache directory removed.")?;
    } else {
        writeln!(stdout, "No cache directory found.")?;
    }
    Ok(ExitStatus::Success)
}
