#[allow(clippy::module_inception)]
mod diagnostic;
mod render;
pub mod reporter;
mod sub_diagnostic;
mod utils;

pub use diagnostic::{
    Diagnostic, DiagnosticErrorType, DiagnosticInner, DiagnosticSeverity,
    TestCaseCollectionDiagnosticType, TestCaseDiagnosticType,
};
pub use sub_diagnostic::{
    FixtureSubDiagnosticType, SubDiagnostic, SubDiagnosticErrorType, SubDiagnosticSeverity,
};
