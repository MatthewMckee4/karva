use crate::{Annotation, Diagnostic, RenderedDiagnostic, Severity};
use annotate_snippets::{Level, Renderer, Snippet};
use camino::{Utf8Path, Utf8PathBuf};
use ruff_source_file::SourceFile;

/// Shape used when rendering diagnostics for users.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticFormat {
    Full,
    Concise,
}

/// Terminal-specific diagnostic rendering settings.
#[derive(Debug, Clone, Copy)]
pub struct DisplayDiagnosticConfig {
    format: DiagnosticFormat,
    color: bool,
}

impl DisplayDiagnosticConfig {
    pub fn new(format: DiagnosticFormat, color: bool) -> Self {
        Self { format, color }
    }
}

/// Renders a worker diagnostic into transport-safe plain and terminal forms.
pub fn render_diagnostic(
    diagnostic: &Diagnostic,
    cwd: &Utf8Path,
    config: DisplayDiagnosticConfig,
) -> RenderedDiagnostic {
    let rendered = render(diagnostic, cwd, config.format, false);
    let colored_rendered = render(diagnostic, cwd, config.format, config.color);
    RenderedDiagnostic::new(
        diagnostic.code(),
        diagnostic.severity(),
        diagnostic.primary_message(),
        rendered,
    )
    .with_colored_rendered(colored_rendered)
}

fn render(
    diagnostic: &Diagnostic,
    cwd: &Utf8Path,
    format: DiagnosticFormat,
    color: bool,
) -> String {
    if format == DiagnosticFormat::Concise {
        return render_concise(diagnostic, cwd);
    }

    let mut rendered = render_message(
        diagnostic.severity(),
        Some(diagnostic.code()),
        diagnostic.primary_message(),
        diagnostic.annotations(),
        cwd,
        color,
    );
    for diagnostic in diagnostic.sub_diagnostics() {
        rendered.push_str(&render_message(
            diagnostic.severity(),
            None,
            diagnostic.message(),
            diagnostic.annotations(),
            cwd,
            color,
        ));
    }
    rendered.push('\n');
    rendered
}

fn render_concise(diagnostic: &Diagnostic, cwd: &Utf8Path) -> String {
    let severity = severity_name(diagnostic.severity());
    if let Some(annotation) = diagnostic.primary_annotation() {
        let span = annotation.span();
        let location = span
            .source_file()
            .to_source_code()
            .line_column(span.range().start());
        format!(
            "{}:{location}: {severity}[{}] {}\n",
            display_path(span.source_file(), cwd),
            diagnostic.code(),
            diagnostic.concise_message()
        )
    } else {
        format!(
            "{severity}[{}] {}\n",
            diagnostic.code(),
            diagnostic.concise_message()
        )
    }
}

fn render_message(
    severity: Severity,
    code: Option<&str>,
    message: &str,
    annotations: &[Annotation],
    cwd: &Utf8Path,
    color: bool,
) -> String {
    let mut files: Vec<(&SourceFile, Vec<&Annotation>)> = Vec::new();
    for annotation in annotations {
        let source_file = annotation.span().source_file();
        if let Some((_, file_annotations)) = files
            .iter_mut()
            .find(|(existing, _)| *existing == source_file)
        {
            file_annotations.push(annotation);
        } else {
            files.push((source_file, vec![annotation]));
        }
    }
    let paths = files
        .iter()
        .map(|(source_file, _)| display_path(source_file, cwd).into_string())
        .collect::<Vec<_>>();
    let snippets = files
        .iter_mut()
        .zip(&paths)
        .flat_map(|((source_file, annotations), path)| {
            let groups = if annotations.windows(2).all(|pair| {
                source_file
                    .to_source_code()
                    .line_column(pair[0].span().range().start())
                    .line
                    == source_file
                        .to_source_code()
                        .line_column(pair[1].span().range().start())
                        .line
            }) {
                annotations.sort_by_key(|annotation| annotation.span().range().start());
                vec![annotations.clone()]
            } else {
                annotations
                    .iter()
                    .map(|annotation| vec![*annotation])
                    .collect()
            };
            groups.into_iter().map(move |annotations| {
                Snippet::source(source_file.source_text())
                    .origin(path)
                    .fold(true)
                    .annotations(annotations.into_iter().map(|annotation| {
                        let range = annotation.span().range();
                        let level = if annotation.is_primary() {
                            Level::Error
                        } else {
                            Level::Warning
                        };
                        let rendered_annotation =
                            level.span(usize::from(range.start())..usize::from(range.end()));
                        if let Some(message) = annotation.message_text() {
                            rendered_annotation.label(message)
                        } else {
                            rendered_annotation
                        }
                    }))
            })
        });
    let mut diagnostic = level(severity).title(message).snippets(snippets);
    if let Some(code) = code {
        diagnostic = diagnostic.id(code);
    }
    let renderer = if color {
        Renderer::styled()
    } else {
        Renderer::plain()
    };
    format!("{}\n", renderer.render(diagnostic))
}

fn display_path(source_file: &SourceFile, cwd: &Utf8Path) -> Utf8PathBuf {
    let path = Utf8Path::new(source_file.name());
    path.strip_prefix(cwd).unwrap_or(path).to_path_buf()
}

fn level(severity: Severity) -> Level {
    match severity {
        Severity::Info => Level::Info,
        Severity::Warning => Level::Warning,
        Severity::Error | Severity::Fatal => Level::Error,
    }
}

fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Info => "info",
        Severity::Warning => "warning",
        Severity::Error => "error",
        Severity::Fatal => "fatal",
    }
}
