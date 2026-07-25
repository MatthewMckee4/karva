use std::path::Path;

use anyhow::{Result, bail};
use camino::Utf8Path;
use karva_diagnostic::RenderedDiagnostic;
use ruff_db::diagnostic::{
    Diagnostic, DisplayDiagnosticConfig, DummyFileResolver, FileResolver, Input, UnifiedFile,
};
use ruff_db::files::File;
use ruff_notebook::NotebookIndex;

/// Karva creates diagnostics from `ruff_source_file::SourceFile` values, not
/// from Ruff's ty/Salsa database. Validate that contract before entering
/// Ruff's renderer so an unsupported span is reported as a cache write error
/// instead of becoming a renderer panic.
pub fn render_diagnostic(
    diagnostic: &Diagnostic,
    cwd: &Utf8Path,
    config: &DisplayDiagnosticConfig,
) -> Result<RenderedDiagnostic> {
    ensure_source_file_spans(diagnostic)?;

    let resolver = DiagnosticFileResolver::new(cwd);
    Ok(RenderedDiagnostic::new(
        diagnostic.id().as_str(),
        diagnostic.severity(),
        diagnostic.primary_message(),
        diagnostic.display(&resolver, config).to_string(),
    ))
}

fn ensure_source_file_spans(diagnostic: &Diagnostic) -> Result<()> {
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
    use ruff_db::diagnostic::{
        Annotation, Diagnostic, DiagnosticId, DisplayDiagnosticConfig, LintName, Severity, Span,
    };
    use ruff_source_file::SourceFileBuilder;
    use ruff_text_size::{TextRange, TextSize};

    use super::*;

    #[test]
    fn renders_source_file_diagnostics() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cwd = Utf8PathBuf::try_from(temp_dir.path().to_path_buf()).unwrap();

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

        let config = DisplayDiagnosticConfig::new("karva").context(0);
        let rendered = render_diagnostic(&diagnostic, &cwd, &config).unwrap();

        assert_eq!(rendered.code(), "test-failure");
        assert_eq!(rendered.message(), "Test `test_example` failed");
        assert!(rendered.rendered().contains("test_sample.py"));
        assert!(rendered.rendered().contains("Test `test_example` failed"));
        assert!(rendered.rendered().contains("def test_example():"));
    }
}
