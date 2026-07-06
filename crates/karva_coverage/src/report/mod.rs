//! Combine per-worker JSON files and produce terminal or machine-readable reports.

pub(crate) mod html;
pub(crate) mod json;
pub(crate) mod shared;
mod terminal;
pub(crate) mod xml;

use anyhow::Result;
use camino::Utf8Path;
use fs_err as fs;

pub use terminal::combine_and_report;
pub use terminal::write_cobertura_xml;
pub use terminal::write_html_report;
pub use terminal::write_json_report;

use self::shared::combine;

fn combined_rows(
    cwd: &Utf8Path,
    files: &[impl AsRef<Utf8Path>],
    show_missing: bool,
) -> Result<Option<(std::path::PathBuf, Vec<shared::FileRow>)>> {
    let combined = combine(files)?;
    if combined.is_empty() {
        return Ok(None);
    }

    let cwd_real = fs::canonicalize(cwd.as_std_path())
        .map(|path| dunce::simplified(&path).to_path_buf())
        .unwrap_or_else(|_| cwd.into());
    let rows = shared::build_rows(&cwd_real, &combined, show_missing);
    Ok(Some((cwd_real, rows)))
}
