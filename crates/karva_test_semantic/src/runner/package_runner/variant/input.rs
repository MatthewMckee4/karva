//! Owned inputs for one concrete test variant.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use camino::Utf8PathBuf;
use pyo3::prelude::*;

use crate::discovery::DiscoveredTestFunction;
use crate::extensions::fixtures::{FixtureId, FixturePlan};
use crate::extensions::tags::RuntimeTags;
use crate::runner::test_iterator::TestVariant;

/// Identity inputs that are fixed before fixture setup.
pub(super) struct VariantIdentity {
    /// Explicit parameter ID supplied by the user.
    pub(super) id: Option<String>,
    /// Position in the function's complete parametrization expansion.
    pub(super) case_index: Option<usize>,
}

/// Fixture graph selected for one variant.
pub(super) struct VariantFixtures {
    /// Compiled fixture arena shared by all attempts.
    pub(super) plan: Rc<FixturePlan>,
    /// Fixtures passed as Python keyword arguments.
    pub(super) dependencies: Rc<[FixtureId]>,
    /// Fixtures run for side effects but omitted from test arguments.
    pub(super) use_dependencies: Rc<[FixtureId]>,
    /// Function-scoped auto-use fixtures.
    pub(super) auto_use: Rc<[FixtureId]>,
}

/// Inputs that remain stable across all attempts for one test variant.
pub(super) struct VariantInput<'test> {
    /// Discovered test definition shared by every attempt.
    pub(super) test: &'test DiscoveredTestFunction,
    /// Parameter values reused when a retry prepares fresh fixtures.
    pub(super) params: HashMap<String, Arc<Py<PyAny>>>,
    /// Identity known before fixture setup.
    pub(super) identity: VariantIdentity,
    /// Fixture graph and selected dependencies.
    pub(super) fixtures: VariantFixtures,
    /// Test, parameter, and fixture tags resolved for this variant.
    pub(super) tags: RuntimeTags,
    /// Module path used to build stable snapshot identity.
    pub(super) module_path: Utf8PathBuf,
}

impl<'test> VariantInput<'test> {
    /// Takes ownership of discovered variant data so retries can reuse it.
    pub(super) fn from_test_variant(variant: TestVariant<'test>) -> Self {
        let module_path = variant.module_path().clone();
        let TestVariant {
            test,
            params,
            id,
            case_index,
            fixture_plan,
            fixture_dependencies,
            use_fixture_dependencies,
            auto_use_fixtures,
            tags,
        } = variant;

        Self {
            test,
            params,
            identity: VariantIdentity { id, case_index },
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
