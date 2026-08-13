use crate::{Annotation, Diagnostic, RenderedDiagnostic, Severity};
use annotate_snippets::{AnnotationKind, Element, Level, Renderer, Snippet};
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
        let message = render_message(
            diagnostic.severity(),
            None,
            diagnostic.message(),
            diagnostic.annotations(),
            cwd,
            color,
        );
        if diagnostic.indentation() == 0 {
            rendered.push_str(&message);
        } else {
            for line in message.split_inclusive('\n') {
                rendered.push_str(&" ".repeat(diagnostic.indentation()));
                rendered.push_str(line);
            }
        }
        if let Some(body) = diagnostic.body_text() {
            rendered.push_str(body);
            if !body.ends_with('\n') {
                rendered.push('\n');
            }
        }
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
                    .path(path)
                    .fold(true)
                    .annotations(annotations.into_iter().map(|annotation| {
                        let range = annotation.span().range();
                        let kind = if annotation.is_primary() {
                            AnnotationKind::Primary
                        } else {
                            AnnotationKind::Context
                        };
                        let rendered_annotation =
                            kind.span(usize::from(range.start())..usize::from(range.end()));
                        if let Some(message) = annotation.message_text() {
                            rendered_annotation.label(message)
                        } else {
                            rendered_annotation
                        }
                    }))
            })
        });
    let level_name = match severity {
        Severity::Info => "info",
        Severity::Warning => "warning",
        Severity::Error | Severity::Fatal => "error",
    };
    let title_prefix_width = level_name.len() + 2 + code.map_or(0, |code| code.len() + 2);
    let continuation_prefix = format!("\n{}", " ".repeat(title_prefix_width));
    let message = message.replace(&continuation_prefix, "\n");
    let mut title = level(severity).primary_title(message);
    if let Some(code) = code {
        title = title.id(code);
    }
    let elements = snippets.map(Element::from).collect::<Vec<_>>();
    let report = [title.elements(elements)];
    let renderer = if color {
        Renderer::styled()
    } else {
        Renderer::plain()
    };
    let rendered = renderer.render(&report);
    let rendered = compact_gutter_padding(&rendered);
    format!("{rendered}\n")
}

fn compact_gutter_padding(rendered: &str) -> String {
    let lines = rendered.lines().collect::<Vec<_>>();
    lines
        .iter()
        .enumerate()
        .filter(|(index, line)| {
            !is_gutter_padding(line)
                || lines
                    .get(index + 1)
                    .is_some_and(|next| is_source_line(next))
        })
        .map(|(_, line)| *line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_source_line(line: &str) -> bool {
    let visible = visible_text(line);
    let visible = visible.trim_start();
    let digits = visible
        .char_indices()
        .take_while(|(_, character)| character.is_ascii_digit())
        .map(|(index, character)| index + character.len_utf8())
        .last()
        .unwrap_or(0);
    digits > 0 && visible[digits..].starts_with(" |")
}

fn is_gutter_padding(line: &str) -> bool {
    visible_text(line).trim() == "|"
}

fn visible_text(line: &str) -> String {
    let mut visible = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(character) = chars.next() {
        if character == '\u{1b}' {
            for escape_character in chars.by_ref() {
                if escape_character.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            visible.push(character);
        }
    }
    visible
}

fn display_path(source_file: &SourceFile, cwd: &Utf8Path) -> Utf8PathBuf {
    let path = Utf8Path::new(source_file.name());
    path.strip_prefix(cwd).unwrap_or(path).to_path_buf()
}

fn level<'a>(severity: Severity) -> Level<'a> {
    match severity {
        Severity::Info => Level::INFO,
        Severity::Warning => Level::WARNING,
        Severity::Error | Severity::Fatal => Level::ERROR,
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

#[cfg(test)]
mod tests {
    use super::{compact_gutter_padding, is_gutter_padding};

    #[test]
    fn keeps_leading_gutter_padding() {
        assert_eq!(
            compact_gutter_padding(
                " --> test.py:4:5\n  |\n4 | def test():\n  | ^^^\n  |\ninfo: failed"
            ),
            " --> test.py:4:5\n  |\n4 | def test():\n  | ^^^\ninfo: failed"
        );
    }

    #[test]
    fn detects_gutter_padding() {
        assert!(is_gutter_padding("  |"));
        assert!(is_gutter_padding("  \u{1b}[1;94m|\u{1b}[0m"));
        assert!(!is_gutter_padding("4 | def test_example():"));
    }
}
