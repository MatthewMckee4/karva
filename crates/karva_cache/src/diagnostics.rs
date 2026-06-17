use std::path::Path;

use anyhow::{Result, bail};
use camino::Utf8Path;
use karva_diagnostic::TestRunResult;
use ruff_db::diagnostic::{
    DisplayDiagnosticConfig, DisplayDiagnostics, DummyFileResolver, FileResolver, Input,
    UnifiedFile,
};
use ruff_db::files::File;
use ruff_notebook::NotebookIndex;

use crate::artifact::{CacheFile, write_text};

/// Renders diagnostics into the worker directory.
///
/// Karva creates diagnostics from `ruff_source_file::SourceFile` values, not
/// from Ruff's ty/Salsa database. Validate that contract before entering
/// Ruff's renderer so an unsupported span is reported as a cache write error
/// instead of becoming a renderer panic.
pub fn write_diagnostics(
    worker_dir: &Utf8Path,
    result: &TestRunResult,
    cwd: &Utf8Path,
    config: &DisplayDiagnosticConfig,
) -> Result<()> {
    if result.diagnostics().is_empty() {
        return Ok(());
    }

    ensure_source_file_spans(result)?;

    let resolver = DiagnosticFileResolver::new(cwd);
    let output = DisplayDiagnostics::new(&resolver, config, result.diagnostics());
    write_text(worker_dir, CacheFile::Diagnostics, output.to_string())
}

fn ensure_source_file_spans(result: &TestRunResult) -> Result<()> {
    for diagnostic in result.diagnostics() {
        for annotation in diagnostic
            .primary_annotation()
            .into_iter()
            .chain(diagnostic.secondary_annotations())
        {
            ensure_source_file_span(annotation.get_span().file())?;
        }

        for sub_diagnostic in diagnostic.sub_diagnostics() {
            for annotation in sub_diagnostic.annotations() {
                ensure_source_file_span(annotation.get_span().file())?;
            }
        }
    }

    Ok(())
}

fn ensure_source_file_span(file: &UnifiedFile) -> Result<()> {
    if matches!(file, UnifiedFile::Ty(_)) {
        bail!("cannot render ty-backed diagnostics without a Ruff database");
    }
    Ok(())
}

struct DiagnosticFileResolver<'a> {
    cwd: &'a Utf8Path,
}

impl<'a> DiagnosticFileResolver<'a> {
    fn new(cwd: &'a Utf8Path) -> Self {
        Self { cwd }
    }
}

impl FileResolver for DiagnosticFileResolver<'_> {
    fn path(&self, file: File) -> &str {
        DummyFileResolver.path(file)
    }

    fn input(&self, file: File) -> Input {
        DummyFileResolver.input(file)
    }

    fn notebook_index(&self, _file: &UnifiedFile) -> Option<NotebookIndex> {
        None
    }

    fn is_notebook(&self, _file: &UnifiedFile) -> bool {
        false
    }

    fn current_directory(&self) -> &Path {
        self.cwd.as_std_path()
    }
}

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;
    use karva_diagnostic::TestRunResult;
    use ruff_db::diagnostic::{
        Annotation, Diagnostic, DiagnosticId, DisplayDiagnosticConfig, LintName, Severity, Span,
    };
    use ruff_source_file::SourceFileBuilder;
    use ruff_text_size::{TextRange, TextSize};

    use super::*;

    #[test]
    fn writes_source_file_diagnostics() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cwd = Utf8PathBuf::try_from(temp_dir.path().to_path_buf()).unwrap();
        let worker_dir = cwd.join("worker-0");
        std::fs::create_dir_all(&worker_dir).unwrap();

        let source = "def test_example():\n    assert False\n";
        let source_file =
            SourceFileBuilder::new(cwd.join("test_sample.py").as_str(), source).finish();
        let mut diagnostic = Diagnostic::new(
            DiagnosticId::Lint(LintName::of("test-failure")),
            Severity::Error,
            "Test `test_example` failed",
        );
        diagnostic.annotate(Annotation::primary(
            Span::from(source_file).with_range(TextRange::new(TextSize::new(4), TextSize::new(16))),
        ));

        let mut result = TestRunResult::default();
        result.add_diagnostic(diagnostic);

        let config = DisplayDiagnosticConfig::new("karva").context(0);
        write_diagnostics(&worker_dir, &result, &cwd, &config).unwrap();

        let rendered =
            std::fs::read_to_string(CacheFile::Diagnostics.path_in(&worker_dir)).unwrap();
        assert!(rendered.contains("test_sample.py"));
        assert!(rendered.contains("Test `test_example` failed"));
        assert!(rendered.contains("def test_example():"));
    }
}
