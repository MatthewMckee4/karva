use ruff_db::diagnostic::{Diagnostic, DiagnosticId, LintName, Severity};

/// Defines a type of diagnostic that can be reported during test execution.
///
/// Each diagnostic type has a unique name, summary description, and severity level
/// that determines how it should be displayed to the user.
#[derive(Debug, Clone)]
pub struct DiagnosticType {
    /// The unique identifier for this diagnostic type.
    pub name: LintName,

    /// A one-sentence summary of what this diagnostic catches.
    #[expect(unused)]
    pub summary: &'static str,

    /// The severity level (error, warning, etc.) of this diagnostic.
    pub(super) severity: Severity,
}

#[macro_export]
macro_rules! declare_diagnostic_type {
    (
        $(#[doc = $doc:literal])+
        $vis: vis static $name: ident = {
            summary: $summary: literal,
            $( $key:ident: $value:expr, )*
        }
    ) => {
        $( #[doc = $doc] )+
        $vis static $name: $crate::diagnostic::metadata::DiagnosticType = $crate::diagnostic::metadata::DiagnosticType {
            name: ruff_db::diagnostic::LintName::of(ruff_macros::kebab_case!($name)),
            summary: $summary,
            $( $key: $value, )*
        };
    };
}

impl DiagnosticType {
    pub(super) fn diagnostic(&self, message: impl std::fmt::Display) -> Diagnostic {
        Diagnostic::new(DiagnosticId::Lint(self.name), self.severity, message)
    }
}
