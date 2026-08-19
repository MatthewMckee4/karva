//! Convert collected modules into partitioning metadata.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use karva_python_semantic::TestCacheKey;

/// Test metadata needed to filter, group, weight, and dispatch one test.
#[derive(Debug, Clone)]
pub(super) struct TestInfo {
    /// Function-level metadata shared by cases collected from one function.
    pub(super) identity: Arc<TestIdentity>,

    /// Qualified name of test, used for last-failed filtering.
    pub(super) qualified_name: String,

    /// Wall-clock runtime from previous run, when cached.
    pub(super) duration: Option<Duration>,

    /// Stable expansion index for a statically countable parameter case.
    pub(super) case_index: Option<usize>,
}

/// Scheduling identity shared by every case collected from one function.
#[derive(Debug)]
pub(super) struct TestIdentity {
    /// Importable module name used to keep cheap modules together.
    pub(super) module_name: Arc<str>,

    /// Worker CLI selector without a statically known parameter-case suffix.
    pub(super) selector: Arc<str>,

    /// Qualified name without a statically known `[idx]` suffix.
    pub(super) function_root: Arc<str>,
}

/// Recursively collect test metadata from package and subpackages.
pub(super) fn collect_test_paths_recursive(
    package: &karva_collector::CollectedPackage,
    test_infos: &mut Vec<TestInfo>,
    previous_durations: &HashMap<TestCacheKey, Duration>,
) {
    for module in package.modules.values() {
        for test_fn_def in &module.test_function_defs {
            let module_name: Arc<str> = module.path.module_name().into();
            let module_path = module.path.path();
            let function_name = test_fn_def.name.as_str();
            let qualified_function: Arc<str> = format!("{module_name}::{function_name}").into();
            let identity = Arc::new(TestIdentity {
                module_name,
                selector: format!("{module_path}::{function_name}").into(),
                function_root: qualified_function,
            });
            let case_count = karva_collector::count_parametrize_cases(test_fn_def);

            if let Some(case_count) = case_count
                && case_count > 0
            {
                for idx in 0..case_count {
                    let qualified_name = format!("{}[{idx}]", identity.function_root);
                    let duration = previous_durations
                        .get(qualified_name.as_str())
                        .copied()
                        .or_else(|| {
                            u32::try_from(case_count).ok().and_then(|case_count| {
                                previous_durations
                                    .get(identity.function_root.as_ref())
                                    .and_then(|duration| duration.checked_div(case_count))
                            })
                        });
                    test_infos.push(TestInfo {
                        identity: Arc::clone(&identity),
                        qualified_name,
                        duration,
                        case_index: Some(idx),
                    });
                }
            } else {
                let duration = previous_durations
                    .get(identity.function_root.as_ref())
                    .copied();
                test_infos.push(TestInfo {
                    qualified_name: identity.function_root.to_string(),
                    identity,
                    duration,
                    case_index: None,
                });
            }
        }

        for doctest in &module.doctests {
            let module_name: Arc<str> = module.path.module_name().into();
            let module_path = module.path.path();
            let function_name = doctest.name.as_str();
            let qualified_function: Arc<str> = format!("{module_name}::{function_name}").into();
            let identity = Arc::new(TestIdentity {
                module_name,
                selector: format!("{module_path}::{function_name}").into(),
                function_root: qualified_function,
            });
            test_infos.push(TestInfo {
                duration: previous_durations
                    .get(identity.function_root.as_ref())
                    .copied(),
                qualified_name: identity.function_root.to_string(),
                identity,
                case_index: None,
            });
        }
    }

    for subpackage in package.packages.values() {
        collect_test_paths_recursive(subpackage, test_infos, previous_durations);
    }
}
