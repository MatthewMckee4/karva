//! Source-backed diagnostics produced while collecting and running tests.

use ruff_source_file::SourceFile;
use ruff_text_size::{TextRange, TextSize};
use serde::{Deserialize, Serialize};

/// Importance of a diagnostic and whether it makes the test run fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
    Fatal,
}

/// Source location highlighted by a diagnostic.
#[derive(Debug, Clone)]
pub struct Span {
    source_file: SourceFile,
    range: TextRange,
}

impl Span {
    #[must_use]
    pub fn with_range(mut self, range: TextRange) -> Self {
        self.range = range;
        self
    }

    pub(super) fn source_file(&self) -> &SourceFile {
        &self.source_file
    }

    pub(super) fn range(&self) -> TextRange {
        self.range
    }
}

impl From<SourceFile> for Span {
    fn from(source_file: SourceFile) -> Self {
        Self {
            source_file,
            range: TextRange::empty(TextSize::default()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnnotationKind {
    Primary,
    Secondary,
}

/// Label attached to a source span.
#[derive(Debug, Clone)]
pub struct Annotation {
    kind: AnnotationKind,
    span: Span,
    message: Option<String>,
}

impl Annotation {
    pub fn primary(span: Span) -> Self {
        Self {
            kind: AnnotationKind::Primary,
            span,
            message: None,
        }
    }

    pub fn secondary(span: Span) -> Self {
        Self {
            kind: AnnotationKind::Secondary,
            span,
            message: None,
        }
    }

    #[must_use]
    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    pub(super) fn is_primary(&self) -> bool {
        self.kind == AnnotationKind::Primary
    }

    pub(super) fn span(&self) -> &Span {
        &self.span
    }

    pub(super) fn message_text(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

/// Supporting diagnostic displayed after the primary diagnostic.
#[derive(Debug, Clone)]
pub struct SubDiagnostic {
    severity: Severity,
    message: String,
    annotations: Vec<Annotation>,
    body: Option<String>,
    indentation: usize,
}

impl SubDiagnostic {
    pub fn new(severity: Severity, message: impl Into<String>) -> Self {
        Self {
            severity,
            message: message.into(),
            annotations: Vec::new(),
            body: None,
            indentation: 0,
        }
    }

    pub fn annotate(&mut self, annotation: Annotation) {
        self.annotations.push(annotation);
    }

    /// Add preformatted content displayed below the diagnostic title.
    pub fn body(&mut self, body: impl Into<String>) {
        self.body = Some(body.into());
    }

    /// Indent every rendered line by `spaces` columns.
    pub fn indent(&mut self, spaces: usize) {
        self.indentation = spaces;
    }

    pub(super) fn severity(&self) -> Severity {
        self.severity
    }

    pub(super) fn message(&self) -> &str {
        &self.message
    }

    pub(super) fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    pub(super) fn body_text(&self) -> Option<&str> {
        self.body.as_deref()
    }

    pub(super) fn indentation(&self) -> usize {
        self.indentation
    }
}

/// Diagnostic value passed from test execution to the cache renderer.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    code: &'static str,
    severity: Severity,
    message: String,
    concise_message: Option<String>,
    annotations: Vec<Annotation>,
    sub_diagnostics: Vec<SubDiagnostic>,
}

impl Diagnostic {
    pub fn new(code: &'static str, severity: Severity, message: impl Into<String>) -> Self {
        Self {
            code,
            severity,
            message: message.into(),
            concise_message: None,
            annotations: Vec::new(),
            sub_diagnostics: Vec::new(),
        }
    }

    pub fn annotate(&mut self, annotation: Annotation) {
        self.annotations.push(annotation);
    }

    pub fn info(&mut self, message: impl Into<String>) {
        self.sub(SubDiagnostic::new(Severity::Info, message));
    }

    pub fn sub(&mut self, diagnostic: SubDiagnostic) {
        self.sub_diagnostics.push(diagnostic);
    }

    pub fn set_concise_message(&mut self, message: impl Into<String>) {
        self.concise_message = Some(message.into());
    }

    pub(super) fn code(&self) -> &'static str {
        self.code
    }

    pub(super) fn severity(&self) -> Severity {
        self.severity
    }

    pub(super) fn primary_message(&self) -> &str {
        &self.message
    }

    pub(super) fn concise_message(&self) -> &str {
        self.concise_message.as_deref().unwrap_or(&self.message)
    }

    pub(super) fn primary_annotation(&self) -> Option<&Annotation> {
        self.annotations
            .iter()
            .find(|annotation| annotation.is_primary())
    }

    pub(super) fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    pub(super) fn sub_diagnostics(&self) -> &[SubDiagnostic] {
        &self.sub_diagnostics
    }
}

/// Sorts source-backed diagnostics into deterministic display order.
pub fn sort_diagnostics_for_display(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by(
        |a, b| match (a.primary_annotation(), b.primary_annotation()) {
            (Some(a), Some(b)) => a
                .span()
                .source_file()
                .cmp(b.span().source_file())
                .then_with(|| a.span().range().start().cmp(&b.span().range().start()))
                .then_with(|| a.span().range().end().cmp(&b.span().range().end())),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a
                .code()
                .cmp(b.code())
                .then_with(|| a.primary_message().cmp(b.primary_message())),
        },
    );
}
