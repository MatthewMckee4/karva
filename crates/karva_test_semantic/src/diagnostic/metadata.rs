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

/// Builder for creating diagnostic guards with the appropriate context.
///
/// Used to construct diagnostics with the correct ID, severity, and context
/// before they are finalized and reported.
pub struct DiagnosticGuardBuilder<'ctx, 'a> {
    /// Reference to the test execution context.
    context: &'ctx Context<'a>,

    /// Unique identifier for this diagnostic.
    id: DiagnosticId,

    /// Severity level for this diagnostic.
    severity: Severity,

    /// Whether this diagnostic occurred during test discovery.
    is_discovery: bool,
}

impl<'ctx, 'a> DiagnosticGuardBuilder<'ctx, 'a> {
    pub(crate) const fn new(
        context: &'ctx Context<'a>,
        diagnostic_type: &'static DiagnosticType,
        is_discovery: bool,
    ) -> Self {
        DiagnosticGuardBuilder {
            context,
            id: DiagnosticId::Lint(diagnostic_type.name),
            severity: diagnostic_type.severity,
            is_discovery,
        }
    }

    /// Build a diagnostic guard with the given message.
    pub(crate) fn into_diagnostic(
        self,
        message: impl std::fmt::Display,
    ) -> DiagnosticGuard<'ctx, 'a> {
        DiagnosticGuard {
            context: self.context,
            diag: Some(Diagnostic::new(self.id, self.severity, message)),
            is_discovery: self.is_discovery,
        }
    }
}

/// A guard that holds a diagnostic and reports it when dropped.
///
/// Allows mutation of the diagnostic before it is automatically
/// reported to the test results when the guard goes out of scope.
pub struct DiagnosticGuard<'ctx, 'a> {
    /// Reference to the test execution context.
    context: &'ctx Context<'a>,

    /// The diagnostic being built, wrapped in Option for take-on-drop.
    diag: Option<Diagnostic>,

    /// Whether this diagnostic occurred during test discovery.
    is_discovery: bool,
}

/// Return a immutable borrow of the diagnostic in this guard.
impl std::ops::Deref for DiagnosticGuard<'_, '_> {
    type Target = Diagnostic;

    fn deref(&self) -> &Diagnostic {
        self.diag.as_ref().unwrap()
    }
}

/// Return a mutable borrow of the diagnostic in this guard.
impl std::ops::DerefMut for DiagnosticGuard<'_, '_> {
    fn deref_mut(&mut self) -> &mut Diagnostic {
        self.diag.as_mut().unwrap()
    }
}

impl Drop for DiagnosticGuard<'_, '_> {
    fn drop(&mut self) {
        let diag = self.diag.take().unwrap();

        if self.is_discovery {
            self.context.result().add_discovery_diagnostic(diag);
        } else {
            self.context.result().add_diagnostic(diag);
        }
    }
}
