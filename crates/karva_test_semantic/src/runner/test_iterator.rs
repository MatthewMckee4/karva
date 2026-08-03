//! Expansion of one discovered test into executable parameter/fixture variants.

use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

use pyo3::prelude::*;

use crate::discovery::DiscoveredTestFunction;
use crate::extensions::fixtures::{FixtureId, FixturePlan, FixtureScope};
use crate::extensions::tags::parametrize::{
    ParameterMetadata, ParameterPlan, ParameterPlanIterator, ParametrizationArgs, ScopedParameter,
    make_unique_parametrize_ids,
};
use crate::extensions::tags::{CompiledTags, RuntimeTags};
use crate::runner::fixture_resolver::{
    FixturePlanCompiler, FixtureResolutionError, FixtureResolutionResult,
};

/// A single variant of a test to be executed.
///
/// Represents one specific invocation of a test function with:
/// - A specific set of parametrize values
/// - Resolved fixture dependencies
/// - Combined tags from the test and parameter set
///
/// The fixture lists are shared between every variant of a test via `Rc<[…]>`,
/// so producing a new variant is a handful of refcount bumps rather than a
/// full `Vec` clone per fixture set.
pub(super) struct TestVariant<'a> {
    /// Reference to the original discovered test function. Borrowed from the
    /// surrounding module, which outlives the iterator.
    pub(super) test: &'a DiscoveredTestFunction,

    /// Parameter values for this variant (from @parametrize). Moved out of
    /// the owning `ParametrizationArgs` so that `Arc::try_unwrap` in the
    /// caller can unwrap without a Python refcount bump.
    pub(super) params: HashMap<String, Arc<Py<PyAny>>>,

    /// Parameter values consumed by fixture `request.param` objects.
    pub(super) fixture_params: HashMap<String, FixtureParameter>,

    /// High-scope parameter cases used only when collection needs reordering.
    pub(super) scoped_params: Option<Box<ScopedParameters>>,

    /// Display and collection identity, boxed off unparametrized variants.
    pub(super) identity: Option<Box<ParameterIdentity>>,

    /// Arena shared by all fixture root groups for this test.
    pub(super) fixture_plan: Rc<FixturePlan>,

    /// Fixtures to be passed as arguments to the test function.
    pub(super) fixture_dependencies: Rc<[FixtureId]>,

    /// Fixtures from @usefixtures (run for side effects, not passed as args).
    pub(super) use_fixture_dependencies: Rc<[FixtureId]>,

    /// Auto-use fixtures that run automatically before this test.
    pub(super) auto_use_fixtures: Rc<[FixtureId]>,

    /// Combined tags from the test and its parameter set.
    pub(super) tags: RuntimeTags,
}

/// One selected fixture parameter for a concrete test variant.
#[derive(Clone, Debug)]
pub(super) struct FixtureParameter {
    pub(super) value: Arc<Py<PyAny>>,
    pub(super) index: usize,
    pub(super) scope: Option<FixtureScope>,
}

/// High-scope cases boxed off the ordinary variant execution path.
pub(super) struct ScopedParameters(Vec<(String, ScopedParameter)>);

/// Parameter identity required by reporting and pytest collection nodes.
pub(super) struct ParameterIdentity {
    pub(super) display: Option<String>,
    pub(super) node: String,
}

impl TestVariant<'_> {
    /// Get the module path for diagnostics.
    pub(super) fn module_path(&self) -> &camino::Utf8PathBuf {
        self.test.name().module_path().path()
    }

    pub(super) fn request_node_id(&self) -> String {
        let name = self.identity.as_ref().map_or_else(
            || self.test.name().function_name().to_string(),
            |identity| format!("{}[{}]", self.test.name().function_name(), identity.node),
        );
        format!("{}::{name}", self.module_path())
    }
}

/// Iterates over all variants of a test function.
///
/// Expands parametrize combinations to produce all concrete test invocations.
/// The iterator borrows the underlying `DiscoveredTestFunction` from the
/// module and shares fixture lists between variants via `Rc<[…]>`, so
/// producing N variants costs N refcount bumps rather than N deep clones.
pub(super) struct TestVariantIterator<'a> {
    /// Discovered Python function shared by all emitted variants.
    test: &'a DiscoveredTestFunction,
    /// Consumed as we iterate, so `values` and `tags` on each
    /// `ParametrizationArgs` are moved into the emitted variant (not cloned).
    param_args: ParameterPlanIterator,
    /// Runtime policy shared by every parameter variant.
    runtime_tags: RuntimeTags,
    /// Resolved fixtures passed as test arguments.
    fixture_plan: Rc<FixturePlan>,
    fixture_dependencies: Rc<[FixtureId]>,
    /// Resolved side-effect-only fixtures.
    use_fixture_dependencies: Rc<[FixtureId]>,
    /// Resolved function-scoped auto-use fixtures.
    auto_use_fixtures: Rc<[FixtureId]>,
}

/// Fixture roots and parameter cases compiled before any test fixture executes.
pub(super) struct CompiledTestPlan {
    fixture_plan: Rc<FixturePlan>,
    fixture_dependencies: Rc<[FixtureId]>,
    use_fixture_dependencies: Rc<[FixtureId]>,
    auto_use_fixtures: Rc<[FixtureId]>,
    parameters: ParameterPlan,
    runtime_tags: RuntimeTags,
    features: PlanFeatures,
}

#[derive(Clone, Copy, Debug, Default)]
struct PlanFeatures(u8);

impl PlanFeatures {
    const USES_REQUEST: u8 = 1;
    const REQUIRES_REORDERING: u8 = 1 << 1;
    const REQUIRES_CROSS_MODULE_REORDERING: u8 = 1 << 2;

    fn new(
        uses_request: bool,
        requires_reordering: bool,
        requires_cross_module_reordering: bool,
    ) -> Self {
        Self(
            u8::from(uses_request)
                | u8::from(requires_reordering) << 1
                | u8::from(requires_cross_module_reordering) << 2,
        )
    }

    fn contains(self, feature: u8) -> bool {
        self.0 & feature != 0
    }
}

impl CompiledTestPlan {
    pub(super) fn requires_reordering(&self) -> bool {
        self.features.contains(PlanFeatures::REQUIRES_REORDERING)
    }

    pub(super) fn requires_cross_module_reordering(&self) -> bool {
        self.features
            .contains(PlanFeatures::REQUIRES_CROSS_MODULE_REORDERING)
    }

    pub(super) fn uses_request(&self) -> bool {
        self.features.contains(PlanFeatures::USES_REQUEST)
    }

    pub(super) fn variant_ids(&self) -> Vec<Option<String>> {
        self.parameters.variant_ids()
    }

    /// Resolves all fixture roots for one test without executing fixture code.
    pub(super) fn compile(
        py: Python,
        test: &DiscoveredTestFunction,
        mut compiler: FixturePlanCompiler<'_>,
    ) -> FixtureResolutionResult<Self> {
        let tags = CompiledTags::new(&test.tags);
        let parametrize_param_names = tags.parameter_names();
        let indirect_names = tags
            .indirect_names()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let has_indirect_parameters = !indirect_names.is_empty();
        if has_indirect_parameters {
            compiler.defer_scope_validation(indirect_names.iter().map(String::as_str));
        }

        let auto_use_fixtures =
            compiler.get_normalized_auto_use_fixtures(py, FixtureScope::Function)?;

        let fixture_dependencies =
            compiler.resolve_test_fixtures(py, test, &parametrize_param_names)?;

        let use_fixture_dependencies =
            compiler.resolve_use_fixtures(py, tags.required_fixtures())?;
        let (mut parameters, runtime_tags) = tags.into_runtime();

        let test_requests_request = test
            .statement()
            .parameters
            .iter_non_variadic_params()
            .any(|parameter| parameter.parameter.name.as_str() == "request");
        let uses_request = test_requests_request || compiler.uses_request();
        if uses_request || has_indirect_parameters {
            compiler.compile_dynamic_fixtures(py);
        }
        let fixture_plan = Rc::new(compiler.finish());
        let mut indirectly_parametrized = HashSet::new();
        let mut missing_indirect = Vec::new();
        for name in &indirect_names {
            if let Some(fixture_id) = fixture_plan.dynamic_fixture(name) {
                indirectly_parametrized.insert(fixture_id);
                let fixture = fixture_plan.fixture(fixture_id);
                parameters.resolve_indirect_scope(name, fixture.scope());
            } else {
                missing_indirect.push(name.clone());
            }
        }
        if !missing_indirect.is_empty() {
            return Err(FixtureResolutionError::MissingTestFixtures {
                definition: Rc::clone(test.definition()),
                missing_fixtures: missing_indirect,
            });
        }
        for (fixture_id, fixture) in fixture_plan.variant_fixtures() {
            if indirectly_parametrized.contains(&fixture_id) {
                continue;
            }
            let Some(fixture_parameters) = fixture.parameters() else {
                continue;
            };
            let name = fixture.name().to_string();
            let mut dimension = fixture_parameters
                .iter()
                .enumerate()
                .map(|(index, parameter)| {
                    ParametrizationArgs::fixture(
                        name.clone(),
                        fixture.function_name(),
                        parameter,
                        index,
                        fixture.scope(),
                    )
                })
                .collect::<Vec<_>>();
            make_unique_parametrize_ids(&mut dimension);
            parameters.push_dimension(dimension);
        }

        let (requires_reordering, requires_cross_module_reordering) =
            parameters.reordering_requirements();
        Ok(Self {
            fixture_plan,
            fixture_dependencies: Rc::from(fixture_dependencies),
            use_fixture_dependencies: Rc::from(use_fixture_dependencies),
            auto_use_fixtures: Rc::from(auto_use_fixtures),
            parameters,
            runtime_tags,
            features: PlanFeatures::new(
                uses_request,
                requires_reordering,
                requires_cross_module_reordering,
            ),
        })
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ScopedParameterKey {
    name: String,
    index: usize,
    path: Option<camino::Utf8PathBuf>,
}

const HIGH_SCOPES: [FixtureScope; 3] = [
    FixtureScope::Session,
    FixtureScope::Package,
    FixtureScope::Module,
];

/// Applies pytest's high-scope parameter grouping to concrete test variants.
pub(super) fn reorder_variants(variants: Vec<TestVariant<'_>>) -> Vec<TestVariant<'_>> {
    if variants.len() < 3 {
        return variants;
    }

    let item_order = reorder_variant_indices(&variants);
    let mut variants = variants.into_iter().map(Some).collect::<Vec<_>>();
    item_order
        .into_iter()
        .filter_map(|item| variants.get_mut(item)?.take())
        .collect()
}

pub(super) fn reorder_variant_indices(variants: &[TestVariant<'_>]) -> Vec<usize> {
    reorder_variant_ref_indices(&variants.iter().collect::<Vec<_>>())
}

pub(super) fn reorder_variant_ref_indices(variants: &[&TestVariant<'_>]) -> Vec<usize> {
    if variants.len() < 3 {
        return (0..variants.len()).collect();
    }

    let mut keys_by_item: [Vec<Vec<ScopedParameterKey>>; 3] =
        std::array::from_fn(|_| vec![Vec::new(); variants.len()]);
    let mut items_by_key: [HashMap<ScopedParameterKey, Vec<usize>>; 3] =
        std::array::from_fn(|_| HashMap::new());

    for (item, variant) in variants.iter().copied().enumerate() {
        let Some(parameters) = variant.scoped_params.as_deref() else {
            continue;
        };
        for (name, parameter) in &parameters.0 {
            let Some(scope_index) = HIGH_SCOPES
                .iter()
                .position(|scope| *scope == parameter.scope)
            else {
                continue;
            };
            let path = match parameter.scope {
                FixtureScope::Session => None,
                FixtureScope::Package => variant
                    .module_path()
                    .parent()
                    .map(camino::Utf8Path::to_path_buf),
                FixtureScope::Module => Some(variant.module_path().clone()),
                FixtureScope::Function => continue,
            };
            let key = ScopedParameterKey {
                name: name.clone(),
                index: parameter.index,
                path,
            };
            if keys_by_item[scope_index][item].contains(&key) {
                continue;
            }
            keys_by_item[scope_index][item].push(key.clone());
            items_by_key[scope_index].entry(key).or_default().push(item);
        }
    }

    reorder_at_scope(
        (0..variants.len()).collect(),
        0,
        &keys_by_item,
        &mut items_by_key,
    )
}

fn reorder_at_scope(
    items: Vec<usize>,
    scope_index: usize,
    keys_by_item: &[Vec<Vec<ScopedParameterKey>>; 3],
    items_by_key: &mut [HashMap<ScopedParameterKey, Vec<usize>>; 3],
) -> Vec<usize> {
    if scope_index == HIGH_SCOPES.len() || items.len() < 3 {
        return items;
    }

    let items_set = items.iter().copied().collect::<HashSet<_>>();
    let mut ignored = HashSet::new();
    let mut remaining = VecDeque::from(items);
    let mut done = Vec::new();
    let mut done_set = HashSet::new();

    while !remaining.is_empty() {
        let mut no_key = Vec::new();
        let mut no_key_set = HashSet::new();
        let mut slicing_key = None;

        while let Some(item) = remaining.pop_front() {
            if done_set.contains(&item) || no_key_set.contains(&item) {
                continue;
            }
            let keys = keys_by_item[scope_index][item]
                .iter()
                .filter(|key| !ignored.contains(*key))
                .collect::<Vec<_>>();
            let Some(key) = keys.last().copied().cloned() else {
                no_key.push(item);
                no_key_set.insert(item);
                continue;
            };

            let matching = items_by_key[scope_index]
                .get(&key)
                .into_iter()
                .flatten()
                .filter(|item| items_set.contains(item))
                .copied()
                .collect::<Vec<_>>();
            for matching_item in matching.into_iter().rev() {
                remaining.push_front(matching_item);
                for other_scope in 0..HIGH_SCOPES.len() {
                    for other_key in &keys_by_item[other_scope][matching_item] {
                        if let Some(key_items) = items_by_key[other_scope].get_mut(other_key)
                            && let Some(position) =
                                key_items.iter().position(|item| *item == matching_item)
                        {
                            let item = key_items.remove(position);
                            key_items.insert(0, item);
                        }
                    }
                }
            }
            slicing_key = Some(key);
            break;
        }

        if !no_key.is_empty() {
            for item in reorder_at_scope(no_key, scope_index + 1, keys_by_item, items_by_key) {
                if done_set.insert(item) {
                    done.push(item);
                }
            }
        }
        if let Some(key) = slicing_key {
            ignored.insert(key);
        }
    }

    done
}

impl<'a> TestVariantIterator<'a> {
    /// Consumes one compiled plan into concrete test variants.
    pub(super) fn new(test: &'a DiscoveredTestFunction, plan: CompiledTestPlan) -> Self {
        Self {
            test,
            param_args: plan.parameters.into_iter(),
            runtime_tags: plan.runtime_tags,
            fixture_plan: plan.fixture_plan,
            fixture_dependencies: plan.fixture_dependencies,
            use_fixture_dependencies: plan.use_fixture_dependencies,
            auto_use_fixtures: plan.auto_use_fixtures,
        }
    }
}

impl<'a> Iterator for TestVariantIterator<'a> {
    type Item = TestVariant<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let param_args = self.param_args.next()?;
        let identity = param_args.node_id().map(|node| {
            Box::new(ParameterIdentity {
                display: param_args.id().map(str::to_string),
                node: node.to_string(),
            })
        });

        let mut params = param_args.values;
        let (indirect_parameters, scoped_params) = param_args.metadata.map_or_else(
            || (None, None),
            |metadata| {
                let ParameterMetadata { indirect, scoped } = *metadata;
                (
                    Some(indirect),
                    (!scoped.is_empty()).then(|| Box::new(ScopedParameters(scoped))),
                )
            },
        );
        let mut fixture_params = HashMap::new();
        for (indirect_name, indirect_parameter) in indirect_parameters.into_iter().flatten() {
            let fixture_name = self
                .fixture_plan
                .dynamic_fixture(&indirect_name)
                .map_or_else(
                    || indirect_name.clone(),
                    |fixture_id| self.fixture_plan.fixture(fixture_id).name().to_string(),
                );
            if let Some(value) = params.remove(&indirect_name) {
                fixture_params.insert(
                    fixture_name,
                    FixtureParameter {
                        value,
                        index: indirect_parameter.index,
                        scope: indirect_parameter.scope,
                    },
                );
            }
        }

        let mut tags = self.runtime_tags.clone();
        tags.extend(&param_args.tags);

        Some(TestVariant {
            test: self.test,
            identity,
            params,
            fixture_params,
            scoped_params,
            fixture_plan: Rc::clone(&self.fixture_plan),
            fixture_dependencies: Rc::clone(&self.fixture_dependencies),
            use_fixture_dependencies: Rc::clone(&self.use_fixture_dependencies),
            auto_use_fixtures: Rc::clone(&self.auto_use_fixtures),
            tags,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.param_args.size_hint()
    }
}
