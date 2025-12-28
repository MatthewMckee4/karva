use anyhow::Result;
use karva_cache::CacheWriter;
use karva_diagnostic::Reporter;
use karva_project::{Db, ProjectDatabase};
use ruff_db::diagnostic::DisplayDiagnosticConfig;

use crate::runner::TestRunner;

pub fn execute_test_paths(
    db: &ProjectDatabase,
    cache_writer: &CacheWriter,
    reporter: &dyn Reporter,
) -> Result<i32> {
    let result = db.test_with_reporter(reporter);

    let diagnostic_format = db.project().settings().terminal().output_format.into();
    let config = DisplayDiagnosticConfig::default()
        .format(diagnostic_format)
        .color(false);

    cache_writer.write_result(&result, db, &config)?;

    Ok(0)
}
