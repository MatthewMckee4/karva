use ruff_db::diagnostic::{Diagnostic, DiagnosticId, LintName, Severity};

use crate::Context;

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
    pub(crate) severity: Severity,
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

/// Builder for reporting diagnostics with the appropriate context.
///
/// Used to construct diagnostics with the correct ID, severity, and context.
pub struct DiagnosticBuilder<'ctx, 'a> {
    /// Reference to the test execution context.
    context: &'ctx Context<'a>,

    /// Unique identifier for this diagnostic.
    id: DiagnosticId,

    /// Severity level for this diagnostic.
    severity: Severity,
}

impl<'ctx, 'a> DiagnosticBuilder<'ctx, 'a> {
    pub(crate) fn new(
        context: &'ctx Context<'a>,
        diagnostic_type: &'static DiagnosticType,
    ) -> Self {
        DiagnosticBuilder {
            context,
            id: DiagnosticId::Lint(diagnostic_type.name),
            severity: diagnostic_type.severity,
        }
    }

    /// Report a diagnostic with the given message.
    pub(crate) fn emit(self, message: impl std::fmt::Display) {
        self.emit_with(message, |_| {});
    }

    /// Report a diagnostic after applying additional annotations or notes.
    pub(crate) fn emit_with(
        self,
        message: impl std::fmt::Display,
        configure: impl FnOnce(&mut Diagnostic),
    ) {
        let mut diagnostic = Diagnostic::new(self.id, self.severity, message);
        configure(&mut diagnostic);
        self.context.result().add_diagnostic(diagnostic);
    }
}

#[cfg(test)]
mod tests {
    use camino::Utf8Path;
    use karva_diagnostic::DummyReporter;
    use karva_metadata::ProjectSettings;
    use ruff_python_ast::PythonVersion;

    use super::*;
    declare_diagnostic_type! {
        /// Test diagnostic.
        static TEST_DIAGNOSTIC = {
            summary: "Test diagnostic",
            severity: Severity::Error,
        }
    }

    #[test]
    fn emit_records_diagnostic() {
        let settings = ProjectSettings::default();
        let reporter = DummyReporter;
        let context = Context::new(
            Utf8Path::new("."),
            &settings,
            PythonVersion::PY312,
            &reporter,
        );

        DiagnosticBuilder::new(&context, &TEST_DIAGNOSTIC).emit("plain diagnostic");

        let result = context.into_result();
        let [diagnostic] = result.diagnostics() else {
            panic!("expected one diagnostic");
        };
        assert_eq!(diagnostic.primary_message(), "plain diagnostic");
    }

    #[test]
    fn emit_with_configures_diagnostic_before_recording() {
        let settings = ProjectSettings::default();
        let reporter = DummyReporter;
        let context = Context::new(
            Utf8Path::new("."),
            &settings,
            PythonVersion::PY312,
            &reporter,
        );

        DiagnosticBuilder::new(&context, &TEST_DIAGNOSTIC).emit_with(
            "diagnostic with context",
            |diagnostic| {
                diagnostic.info("extra context");
                diagnostic.set_concise_message("concise context");
            },
        );

        let result = context.into_result();
        let [diagnostic] = result.diagnostics() else {
            panic!("expected one diagnostic");
        };
        assert_eq!(diagnostic.primary_message(), "diagnostic with context");
        assert_eq!(diagnostic.sub_diagnostics().len(), 1);
        assert_eq!(diagnostic.concise_message().to_string(), "concise context");
    }
}
