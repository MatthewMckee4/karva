//! Owned inputs for one concrete test variant.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use camino::Utf8PathBuf;
use karva_diagnostic::Diagnostic;
use karva_python_semantic::ParameterCase;
use pyo3::prelude::*;

use crate::discovery::DiscoveredTestFunction;
use crate::extensions::fixtures::{FixtureId, FixturePlan};
use crate::extensions::tags::RuntimeTags;
use crate::extensions::tags::parametrize::FixtureParameter;
use crate::runner::test_iterator::TestVariant;

/// Identity inputs that are fixed before fixture setup.
pub(in super::super) struct VariantIdentity {
    /// Explicit parameter ID supplied by the user.
    pub(in super::super) id: Option<String>,
    /// Position in the function's complete parametrization expansion.
    pub(in super::super) case: Option<ParameterCase>,
    pub(in super::super) request_id: Option<String>,
}

/// Fixture graph selected for one variant.
pub(in super::super) struct VariantFixtures {
    /// Compiled fixture arena shared by all attempts.
    pub(in super::super) plan: Rc<FixturePlan>,
    /// Fixtures passed as Python keyword arguments.
    pub(in super::super) dependencies: Rc<[FixtureId]>,
    /// Fixtures run for side effects but omitted from test arguments.
    pub(in super::super) use_dependencies: Rc<[FixtureId]>,
    /// Function-scoped auto-use fixtures.
    pub(in super::super) auto_use: Rc<[FixtureId]>,
}

/// Inputs that remain stable across all attempts for one test variant.
pub(in super::super) struct VariantInput<'test> {
    /// Fixture validation failure reported only for selected variants.
    pub(in super::super) fixture_error: Option<Box<Diagnostic>>,

    /// Discovered test definition shared by every attempt.
    pub(in super::super) test: &'test DiscoveredTestFunction,
    /// Parameter values reused when a retry prepares fresh fixtures.
    pub(in super::super) params: HashMap<String, Arc<Py<PyAny>>>,
    pub(in super::super) indirect: HashMap<String, FixtureParameter>,
    pub(in super::super) case_markers: Vec<Arc<Py<PyAny>>>,
    /// Identity known before fixture setup.
    pub(in super::super) identity: VariantIdentity,
    /// Fixture graph and selected dependencies.
    pub(in super::super) fixtures: VariantFixtures,
    /// Test, parameter, and fixture tags resolved for this variant.
    pub(in super::super) tags: RuntimeTags,
    /// Module path used to build stable snapshot identity.
    pub(in super::super) module_path: Utf8PathBuf,
}

impl<'test> VariantInput<'test> {
    /// Takes ownership of discovered variant data so retries can reuse it.
    pub(in super::super) fn from_test_variant(variant: TestVariant<'test>) -> Self {
        let module_path = variant.module_path().clone();
        let TestVariant {
            fixture_error,
            test,
            params,
            indirect,
            id,
            case,
            request_id,
            case_markers,
            fixture_plan,
            fixture_dependencies,
            use_fixture_dependencies,
            auto_use_fixtures,
            tags,
        } = variant;

        Self {
            fixture_error,
            test,
            params,
            indirect,
            case_markers,
            identity: VariantIdentity {
                id,
                case,
                request_id,
            },
            fixtures: VariantFixtures {
                plan: fixture_plan,
                dependencies: fixture_dependencies,
                use_dependencies: use_fixture_dependencies,
                auto_use: auto_use_fixtures,
            },
            tags,
            module_path,
        }
    }
}
