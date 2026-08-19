use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use camino::{Utf8Path, Utf8PathBuf};
use fs_err as fs;
use karva_collector::{CollectedModule, CollectedPackage};
use karva_project::path::{TestPath, TestPathError, TestPathFunction};
use karva_python_semantic::ModulePath;
use pyo3::prelude::*;
use ruff_python_ast::{PythonVersion, Stmt};
use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};

use crate::Context;
use crate::collection::TestFunctionCollector;
use crate::discovery::visitor::{discover, is_generator};
use crate::discovery::{
    DiscoveredModule, DiscoveredPackage, DiscoveryError, DiscoveryIssue, DiscoveryOutput,
};
use crate::extensions::fixtures::{DiscoveredFixture, RejectedFixture};
use crate::extensions::tags::validation::unknown_tags;
use crate::utils::add_to_sys_path;

/// Maps `(file path, function name)` to the parametrize indices the worker
/// should run for that function.
///
/// `None` means the function appeared without an `[idx]` suffix at least once,
/// so every case should run. `Some(indices)` means only those indices.
type CaseFilterMap = HashMap<(Utf8PathBuf, String), Option<Vec<usize>>>;

/// Discovers test functions and fixtures from Python source files.
///
/// Handles the conversion from collected AST information to fully discovered
/// test entities by importing Python modules and resolving function references.
pub struct StandardDiscoverer<'ctx, 'a> {
    /// Reference to the test execution context.
    context: &'ctx Context<'a>,

    /// Ordered issues produced across all modules in the package tree.
    issues: Vec<DiscoveryIssue>,
}

impl<'ctx, 'a> StandardDiscoverer<'ctx, 'a> {
    pub fn new(context: &'ctx Context<'a>) -> Self {
        Self {
            context,
            issues: Vec::new(),
        }
    }

    pub(crate) fn discover_with_py(
        mut self,
        py: Python<'_>,
        test_paths: Vec<Result<TestPath, TestPathError>>,
    ) -> DiscoveryOutput {
        let cwd = self.context.cwd();

        if add_to_sys_path(py, cwd, 0).is_err() {
            return DiscoveryOutput {
                package: DiscoveredPackage::new(cwd.to_path_buf()),
                issues: self.issues,
            };
        }

        let test_paths: Vec<TestPathFunction> = test_paths
            .into_iter()
            .filter_map(|path| match path {
                Ok(path) => match path {
                    TestPath::Directory(_) | TestPath::File(_) => None,
                    TestPath::Function(function) => Some(function),
                },
                Err(_) => None,
            })
            .collect();

        let case_filter = build_case_filter(&test_paths);

        let collector =
            TestFunctionCollector::new(self.context.cwd(), self.context.collection_settings());

        let collected_package = match collector.collect_all(test_paths) {
            Ok(package) => package,
            Err(error) => {
                self.issues
                    .push(DiscoveryIssue::Error(DiscoveryError::Collection(error)));
                return DiscoveryOutput {
                    package: DiscoveredPackage::new(cwd.to_path_buf()),
                    issues: self.issues,
                };
            }
        };

        let mut session_package = self.convert_package(py, collected_package, &case_filter);

        session_package.shrink();

        session_package.set_framework_module(discover_framework_fixtures(
            py,
            self.context.python_version(),
        ));

        DiscoveryOutput {
            package: session_package,
            issues: self.issues,
        }
    }

    /// Convert a collected package to a discovered package by importing Python modules
    /// and resolving test functions and fixtures.
    fn convert_package(
        &mut self,
        py: Python,
        collected_package: CollectedPackage,
        case_filter: &CaseFilterMap,
    ) -> DiscoveredPackage {
        let CollectedPackage {
            path,
            modules,
            packages,
            configuration_module,
        } = collected_package;

        let mut discovered_package = DiscoveredPackage::new(path);

        if let Some(collected_module) = configuration_module {
            discovered_package.set_configuration_module(Some(self.convert_module(
                py,
                collected_module,
                case_filter,
            )));
        }

        for collected_module in modules.into_values() {
            discovered_package.add_direct_module(self.convert_module(
                py,
                collected_module,
                case_filter,
            ));
        }

        for collected_subpackage in packages.into_values() {
            discovered_package.add_direct_subpackage(self.convert_package(
                py,
                collected_subpackage,
                case_filter,
            ));
        }

        discovered_package
    }

    fn convert_module(
        &mut self,
        py: Python,
        collected_module: CollectedModule,
        case_filter: &CaseFilterMap,
    ) -> DiscoveredModule {
        let CollectedModule {
            path,
            module_type: _,
            source_text,
            module_body,
            test_function_defs,
            doctests,
            fixture_function_defs,
        } = collected_module;

        let module_file_path = path.path().clone();
        let mut module = DiscoveredModule::new_with_source(path, source_text);

        if self.context.settings().test().strict_tags {
            let mut has_unknown_tags = false;
            for function in test_function_defs.iter().chain(&fixture_function_defs) {
                for unknown in unknown_tags(function, self.context.settings().tags()) {
                    self.issues
                        .push(DiscoveryIssue::Error(DiscoveryError::UnknownTag {
                            source_file: module.source_file(),
                            name: unknown.name,
                            range: unknown.range,
                            suggestion: unknown.suggestion,
                        }));
                    has_unknown_tags = true;
                }
            }
            if has_unknown_tags {
                return module;
            }
        }

        let test_function_defs: Vec<_> = if case_filter.is_empty() {
            test_function_defs
                .into_iter()
                .map(|def| (def, None))
                .collect()
        } else {
            test_function_defs
                .into_iter()
                .map(|def| {
                    let key = (module_file_path.clone(), def.name.to_string());
                    let filter = case_filter.get(&key).cloned().unwrap_or(None);
                    (def, filter)
                })
                .collect()
        };

        self.issues.extend(discover(
            self.context,
            py,
            &mut module,
            module_body,
            test_function_defs,
            doctests,
            fixture_function_defs,
        ));

        module
    }
}

/// Build a `(file path, function name) -> Option<Vec<usize>>` map from the
/// resolved test path selectors. `None` means "run every parametrize case",
/// `Some(indices)` means "run only these case indices."
///
/// Multiple selectors for the same function are unioned: any bare selector
/// (no `[idx]`) wins and yields `None`; otherwise the indices are merged.
fn build_case_filter(test_paths: &[TestPathFunction]) -> CaseFilterMap {
    if test_paths
        .iter()
        .all(|test_path| test_path.parametrize_index.is_none())
    {
        return CaseFilterMap::new();
    }

    let mut filter: CaseFilterMap = HashMap::new();

    for test_path in test_paths {
        let key = (test_path.path.clone(), test_path.function_name.clone());
        match filter.get_mut(&key) {
            Some(None) => {}
            Some(existing @ Some(_)) if test_path.parametrize_index.is_none() => *existing = None,
            Some(Some(indices)) => {
                if let Some(index) = test_path.parametrize_index {
                    indices.push(index);
                }
            }
            None => {
                filter.insert(key, test_path.parametrize_index.map(|index| vec![index]));
            }
        }
    }

    for indices in filter.values_mut().flatten() {
        indices.sort_unstable();
        indices.dedup();
    }

    filter
}

/// Discovers all fixtures defined in `karva._builtins` by importing the module at
/// runtime and parsing its source file.
///
/// Returns a synthetic `DiscoveredModule` holding the discovered fixtures, or
/// `None` if `karva._builtins` cannot be imported or parsed. The returned
/// module is intended to be attached to the session root's `framework_module`
/// slot so that fixture resolution walks through it via `HasFixtures`.
///
/// Any failure to locate, read, or parse the module is logged at warn level
/// so users who end up with an empty framework module (and thus "fixture not
/// found" errors for `tmp_path`, `monkeypatch`, etc.) can trace the cause.
fn discover_framework_fixtures(
    py: Python<'_>,
    python_version: PythonVersion,
) -> Option<DiscoveredModule> {
    let builtins_module = match py.import("karva._builtins") {
        Ok(module) => module,
        Err(err) => {
            tracing::warn!("Failed to import `karva._builtins`: {err}");
            return None;
        }
    };

    let file_path_obj = match builtins_module.getattr("__file__") {
        Ok(obj) => obj,
        Err(err) => {
            tracing::warn!("`karva._builtins` is missing a `__file__` attribute: {err}");
            return None;
        }
    };
    let file_path_str: String = match file_path_obj.extract() {
        Ok(path) => path,
        Err(err) => {
            tracing::warn!("`karva._builtins.__file__` is not a string: {err}");
            return None;
        }
    };
    let Some(utf8_path) = Utf8Path::from_path(Path::new(&file_path_str)) else {
        tracing::warn!("`karva._builtins.__file__` ({file_path_str}) is not valid UTF-8");
        return None;
    };

    let source_text = match fs::read_to_string(utf8_path) {
        Ok(text) => text,
        Err(err) => {
            tracing::warn!("Failed to read `karva._builtins` source at {utf8_path}: {err}");
            return None;
        }
    };

    let module_path = ModulePath::new_with_name(utf8_path, "karva._builtins".to_string());

    let mut parse_options = ParseOptions::from(Mode::Module);
    parse_options = parse_options.with_target_version(python_version);
    let Some(parsed) = parse_unchecked(&source_text, parse_options).try_into_module() else {
        tracing::warn!("Failed to parse `karva._builtins` as a Python module");
        return None;
    };

    let mut framework_module = DiscoveredModule::new_with_source(module_path.clone(), source_text);

    for stmt in parsed.into_syntax().body {
        let Stmt::FunctionDef(function_def) = stmt else {
            continue;
        };
        if function_def.name.starts_with('_') {
            continue;
        }
        let fixture_name = function_def.name.to_string();
        let is_gen = is_generator(&function_def);
        let stmt_rc = Rc::new(function_def);
        match DiscoveredFixture::try_from_function(
            py,
            Rc::clone(&stmt_rc),
            &builtins_module,
            &module_path,
            framework_module.source_file(),
            is_gen,
        ) {
            Ok(fixture) => framework_module.add_fixture(fixture),
            Err(err) => {
                let source_file = framework_module.source_file();
                framework_module.add_rejected_fixture(RejectedFixture::new(
                    fixture_name.clone(),
                    err.value(py).to_string(),
                    stmt_rc,
                    source_file,
                    module_path.clone(),
                ));
                tracing::warn!(
                    "Failed to discover framework fixture `{fixture_name}` from `karva._builtins`: {err}"
                );
            }
        }
    }

    Some(framework_module)
}

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;
    use karva_project::path::TestPathFunction;

    use super::build_case_filter;

    fn test_path(parametrize_index: Option<usize>) -> TestPathFunction {
        TestPathFunction {
            path: Utf8PathBuf::from("tests/test_example.py"),
            function_name: "test_example".to_string(),
            parametrize_index,
        }
    }

    #[test]
    fn sorts_and_deduplicates_case_indices() {
        let filter = build_case_filter(&[
            test_path(Some(3)),
            test_path(Some(1)),
            test_path(Some(3)),
            test_path(Some(2)),
        ]);

        assert_eq!(
            filter.get(&(
                Utf8PathBuf::from("tests/test_example.py"),
                "test_example".to_string(),
            )),
            Some(&Some(vec![1, 2, 3]))
        );
    }

    #[test]
    fn bare_selector_keeps_all_case_indices() {
        let filter = build_case_filter(&[test_path(Some(2)), test_path(None), test_path(Some(1))]);

        assert_eq!(
            filter.get(&(
                Utf8PathBuf::from("tests/test_example.py"),
                "test_example".to_string(),
            )),
            Some(&None)
        );
    }
}
