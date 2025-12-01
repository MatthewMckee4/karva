use ruff_db::diagnostic::{Diagnostic, DiagnosticId, LintName, Severity};

use crate::Context;

#[derive(Debug, Clone)]
pub struct DiagnosticType {
    /// The unique identifier for the rule.
    pub name: LintName,

    /// A one-sentence summary of what the rule catches.
    #[expect(unused)]
    pub summary: &'static str,

    /// The level of the diagnostic.
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

pub struct DiagnosticGuardBuilder<'ctx, 'proj, 'rep> {
    context: &'ctx Context<'proj, 'rep>,
    id: DiagnosticId,
    severity: Severity,
    is_discovery: bool,
}

impl<'ctx, 'proj, 'rep> DiagnosticGuardBuilder<'ctx, 'proj, 'rep> {
    pub(crate) const fn new(
        context: &'ctx Context<'proj, 'rep>,
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
    ) -> DiagnosticGuard<'ctx, 'proj, 'rep> {
        DiagnosticGuard {
            context: self.context,
            diag: Some(Diagnostic::new(self.id, self.severity, message)),
            is_discovery: self.is_discovery,
        }
    }
}

/// An abstraction for mutating a diagnostic.
pub struct DiagnosticGuard<'ctx, 'proj, 'rep> {
    context: &'ctx Context<'proj, 'rep>,

    diag: Option<Diagnostic>,

    is_discovery: bool,
}

/// Return a immutable borrow of the diagnostic in this guard.
impl std::ops::Deref for DiagnosticGuard<'_, '_, '_> {
    type Target = Diagnostic;

    fn deref(&self) -> &Diagnostic {
        self.diag.as_ref().unwrap()
    }
}

/// Return a mutable borrow of the diagnostic in this guard.
impl std::ops::DerefMut for DiagnosticGuard<'_, '_, '_> {
    fn deref_mut(&mut self) -> &mut Diagnostic {
        self.diag.as_mut().unwrap()
    }
}

impl Drop for DiagnosticGuard<'_, '_, '_> {
    fn drop(&mut self) {
        let diag = self.diag.take().unwrap();

        if self.is_discovery {
            self.context.result().add_discovery_diagnostic(diag);
        } else {
            self.context.result().add_diagnostic(diag);
        }
    }
}
