//! Convert collected modules into partitioning metadata.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use karva_python_semantic::TestCacheKey;

/// Test metadata needed to filter, group, weight, and dispatch one test.
#[derive(Debug, Clone)]
pub(super) struct TestInfo {
    /// Importable module name used to keep cheap modules together.
    pub(super) module_name: String,

    /// Qualified name of test, used for last-failed filtering.
    pub(super) qualified_name: String,

    /// Worker CLI selector for this exact test.
    pub(super) path: Arc<str>,

    /// Wall-clock runtime from previous run, when cached.
    pub(super) duration: Option<Duration>,

    /// Qualified name without `[idx]` suffix.
    pub(super) function_root: String,
}

/// Recursively collect test metadata from package and subpackages.
pub(super) fn collect_test_paths_recursive(
    package: &karva_collector::CollectedPackage,
    test_infos: &mut Vec<TestInfo>,
    previous_durations: &HashMap<TestCacheKey, Duration>,
) {
    for module in package.modules.values() {
        for test_fn_def in &module.test_function_defs {
            let module_name = module.path.module_name();
            let module_path = module.path.path();
            let function_name = test_fn_def.name.as_str();
            let function_root = format!("{module_name}::{function_name}");
            let case_count = karva_collector::count_parametrize_cases(test_fn_def);

            if let Some(case_count) = case_count
                && case_count > 0
            {
                for idx in 0..case_count {
                    let qualified_name = format!("{function_root}[{idx}]");
                    let duration = previous_durations
                        .get(qualified_name.as_str())
                        .copied()
                        .or_else(|| {
                            u32::try_from(case_count).ok().and_then(|case_count| {
                                previous_durations
                                    .get(function_root.as_str())
                                    .and_then(|duration| duration.checked_div(case_count))
                            })
                        });
                    test_infos.push(TestInfo {
                        module_name: module_name.to_string(),
                        qualified_name,
                        path: format!("{module_path}::{function_name}[{idx}]").into(),
                        duration,
                        function_root: function_root.clone(),
                    });
                }
            } else {
                let duration = previous_durations.get(function_root.as_str()).copied();
                test_infos.push(TestInfo {
                    module_name: module_name.to_string(),
                    qualified_name: function_root.clone(),
                    path: format!("{module_path}::{function_name}").into(),
                    duration,
                    function_root,
                });
            }
        }

        for doctest in &module.doctests {
            let module_name = module.path.module_name();
            let module_path = module.path.path();
            let function_name = doctest.name.as_str();
            let qualified_name = format!("{module_name}::{function_name}");
            test_infos.push(TestInfo {
                module_name: module_name.to_string(),
                duration: previous_durations.get(qualified_name.as_str()).copied(),
                path: format!("{module_path}::{function_name}").into(),
                function_root: qualified_name.clone(),
                qualified_name,
            });
        }
    }

    for subpackage in package.packages.values() {
        collect_test_paths_recursive(subpackage, test_infos, previous_durations);
    }
}
