use std::path::Path;
use std::rc::Rc;

use camino::Utf8Path;
use karva_collector::{CollectedModule, CollectedPackage};
use karva_project::path::{TestPath, TestPathError};
use karva_python_semantic::ModulePath;
use pyo3::prelude::*;
use ruff_python_ast::{PythonVersion, Stmt};
use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};

use crate::Context;
use crate::collection::TestFunctionCollector;
use crate::diagnostic::report_collection_error;
use crate::discovery::visitor::{discover, is_generator};
use crate::discovery::{DiscoveredModule, DiscoveredPackage};
use crate::extensions::fixtures::DiscoveredFixture;
use crate::utils::add_to_sys_path;

/// Discovers test functions and fixtures from Python source files.
///
/// Handles the conversion from collected AST information to fully discovered
/// test entities by importing Python modules and resolving function references.
pub struct StandardDiscoverer<'ctx, 'a> {
    /// Reference to the test execution context.
    context: &'ctx Context<'a>,
}

impl<'ctx, 'a> StandardDiscoverer<'ctx, 'a> {
    pub fn new(context: &'ctx Context<'a>) -> Self {
        Self { context }
    }

    pub(crate) fn discover_with_py(
        self,
        py: Python<'_>,
        test_paths: Vec<Result<TestPath, TestPathError>>,
    ) -> DiscoveredPackage {
        let cwd = self.context.cwd();

        if add_to_sys_path(py, cwd, 0).is_err() {
            return DiscoveredPackage::new(cwd.to_path_buf());
        }

        let test_paths = test_paths
            .into_iter()
            .filter_map(|path| match path {
                Ok(path) => match path {
                    TestPath::Directory(_) | TestPath::File(_) => None,
                    TestPath::Function(function) => Some(function),
                },
                Err(_) => None,
            })
            .collect();

        let collector =
            TestFunctionCollector::new(self.context.cwd(), self.context.collection_settings());

        let collected_package = match collector.collect_all(test_paths) {
            Ok(package) => package,
            Err(error) => {
                report_collection_error(self.context, &error);
                return DiscoveredPackage::new(cwd.to_path_buf());
            }
        };

        let mut session_package = self.convert_package(py, collected_package);

        session_package.shrink();

        session_package.set_framework_module(discover_framework_fixtures(
            py,
            self.context.python_version(),
        ));

        session_package
    }

    /// Convert a collected package to a discovered package by importing Python modules
    /// and resolving test functions and fixtures.
    fn convert_package(
        &self,
        py: Python,
        collected_package: CollectedPackage,
    ) -> DiscoveredPackage {
        let CollectedPackage {
            path,
            modules,
            packages,
            configuration_module,
        } = collected_package;

        let mut discovered_package = DiscoveredPackage::new(path);

        if let Some(collected_module) = configuration_module {
            discovered_package
                .set_configuration_module(Some(self.convert_module(py, collected_module)));
        }

        for collected_module in modules.into_values() {
            discovered_package.add_direct_module(self.convert_module(py, collected_module));
        }

        for collected_subpackage in packages.into_values() {
            discovered_package
                .add_direct_subpackage(self.convert_package(py, collected_subpackage));
        }

        discovered_package
    }

    fn convert_module(&self, py: Python, collected_module: CollectedModule) -> DiscoveredModule {
        let CollectedModule {
            path,
            module_type: _,
            source_text,
            test_function_defs,
            fixture_function_defs,
        } = collected_module;

        let mut module = DiscoveredModule::new_with_source(path, source_text);

        discover(
            self.context,
            py,
            &mut module,
            test_function_defs,
            fixture_function_defs,
        );

        module
    }
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

    let source_text = match std::fs::read_to_string(utf8_path) {
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
            stmt_rc,
            &builtins_module,
            &module_path,
            framework_module.source_file(),
            is_gen,
        ) {
            Ok(fixture) => framework_module.add_fixture(fixture),
            Err(err) => {
                tracing::warn!(
                    "Failed to discover framework fixture `{fixture_name}` from `karva._builtins`: {err}"
                );
            }
        }
    }

    Some(framework_module)
}
