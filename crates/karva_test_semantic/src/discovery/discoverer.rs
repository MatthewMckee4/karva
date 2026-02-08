use karva_collector::{CollectedModule, CollectedPackage};
use karva_project::path::{TestPath, TestPathError};
use pyo3::prelude::*;

use crate::Context;
use crate::collection::TestFunctionCollector;
use crate::diagnostic::report_invalid_path;
use crate::discovery::visitor::discover;
use crate::discovery::{DiscoveredModule, DiscoveredPackage};
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
    pub const fn new(context: &'ctx Context<'a>) -> Self {
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
                Err(error) => {
                    report_invalid_path(self.context, &error);
                    None
                }
            })
            .collect();

        let collector =
            TestFunctionCollector::new(self.context.cwd(), self.context.collection_settings());

        let collected_package = collector.collect_all(test_paths);

        let mut session_package = self.convert_package(py, collected_package);

        session_package.shrink();

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

        let mut module = DiscoveredModule::new_with_source(path.clone(), source_text);

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
