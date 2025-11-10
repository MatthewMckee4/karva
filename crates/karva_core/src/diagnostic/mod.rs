#[allow(clippy::module_inception)]
pub mod diagnostic;
pub mod render;
pub mod reporter;
pub mod traceback;

pub(crate) use diagnostic::{
    Diagnostic, DiscoveryDiagnostic, InvalidFixtureDiagnostic, MissingFixturesDiagnostic,
    TestFailureDiagnostic, WarningDiagnostic,
};
