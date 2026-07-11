//! Combine per-worker JSON files and produce terminal or machine-readable reports.

pub(crate) mod coveragepy;
pub(crate) mod html;
pub(crate) mod json;
pub(crate) mod shared;
mod terminal;
pub(crate) mod xml;

use anyhow::Result;
use camino::Utf8Path;
use fs_err as fs;
use globset::{Glob, GlobSet, GlobSetBuilder};

pub use coveragepy::write_coveragepy_sqlite;
pub use terminal::combine_and_report;
pub use terminal::write_cobertura_xml;
pub use terminal::write_html_report;
pub use terminal::write_json_report;

use self::shared::combine;

#[derive(Debug, Default)]
pub struct CoverageFilters {
    include: Option<GlobSet>,
    omit: Option<GlobSet>,
}

impl CoverageFilters {
    pub fn new(include: &[String], omit: &[String]) -> Result<Self> {
        Ok(Self {
            include: compile_globs("include", include)?,
            omit: compile_globs("omit", omit)?,
        })
    }

    fn matches(&self, path: &str) -> bool {
        self.include
            .as_ref()
            .is_none_or(|include| include.is_match(path))
            && !self.omit.as_ref().is_some_and(|omit| omit.is_match(path))
    }
}

fn compile_globs(kind: &str, patterns: &[String]) -> Result<Option<GlobSet>> {
    if patterns.is_empty() {
        return Ok(None);
    }

    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(
            Glob::new(pattern).map_err(|err| {
                anyhow::anyhow!("invalid coverage {kind} glob `{pattern}`: {err}")
            })?,
        );
    }
    Ok(Some(builder.build()?))
}

fn combined_rows(
    cwd: &Utf8Path,
    files: &[impl AsRef<Utf8Path>],
    show_missing: bool,
    filters: &CoverageFilters,
) -> Result<Option<(std::path::PathBuf, Vec<shared::FileRow>)>> {
    let combined = combine(files)?;
    if combined.is_empty() {
        return Ok(None);
    }

    let cwd_real = fs::canonicalize(cwd.as_std_path())
        .map(|path| dunce::simplified(&path).to_path_buf())
        .unwrap_or_else(|_| cwd.into());
    let rows = shared::build_rows(&cwd_real, &combined, show_missing)
        .into_iter()
        .filter(|row| filters.matches(&row.name))
        .collect();
    Ok(Some((cwd_real, rows)))
}

#[cfg(test)]
mod tests {
    use super::CoverageFilters;

    #[test]
    fn filters_apply_include_then_omit() {
        let include = vec!["src/**".to_string()];
        let omit = vec!["**/generated.py".to_string()];
        let filters = CoverageFilters::new(&include, &omit).expect("valid filters");

        assert!(filters.matches("src/package/module.py"));
        assert!(!filters.matches("tests/test_module.py"));
        assert!(!filters.matches("src/package/generated.py"));
    }

    #[test]
    fn filters_reject_invalid_globs() {
        let include = vec!["[".to_string()];
        let err = CoverageFilters::new(&include, &[]).expect_err("invalid glob");

        assert!(
            err.to_string()
                .contains("invalid coverage include glob `[`"),
            "{err:?}"
        );
    }
}
