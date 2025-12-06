use pyo3::prelude::*;

use crate::{
    Context,
    collection::{CollectedModule, CollectedPackage, ModuleType, ParallelCollector},
    discovery::{DiscoveredModule, DiscoveredPackage, visitor::discover},
    utils::add_to_sys_path,
};

pub struct StandardDiscoverer<'ctx, 'proj, 'rep> {
    context: &'ctx Context<'proj, 'rep>,
}

impl<'ctx, 'proj, 'rep> StandardDiscoverer<'ctx, 'proj, 'rep> {
    pub const fn new(context: &'ctx Context<'proj, 'rep>) -> Self {
        Self { context }
    }

    pub(crate) fn discover_with_py(self, py: Python<'_>) -> DiscoveredPackage {
        let cwd = self.context.project().cwd();

        if add_to_sys_path(py, cwd, 0).is_err() {
            return DiscoveredPackage::new(cwd.clone());
        }

        tracing::info!("Collecting test files in parallel...");

        let collector = ParallelCollector::new(self.context);
        let collected_package = collector.collect_all();

        tracing::info!("Discovering test functions and fixtures...");

        let mut session_package = self.convert_collected_to_discovered(py, collected_package);

        session_package.shrink();

        session_package
    }

    /// Convert a collected package to a discovered package by importing Python modules
    /// and resolving test functions and fixtures.
    fn convert_collected_to_discovered(
        &self,
        py: Python<'_>,
        collected_package: CollectedPackage,
    ) -> DiscoveredPackage {
        let CollectedPackage {
            path,
            modules,
            packages,
            configuration_module_path: _,
        } = collected_package;

        let mut discovered_package = DiscoveredPackage::new(path);

        // Convert all modules
        for collected_module in modules.into_values() {
            let CollectedModule {
                path,
                module_type,
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

            if module_type == ModuleType::Configuration {
                discovered_package.add_configuration_module(module);
            } else {
                discovered_package.add_direct_module(module);
            }
        }

        // Convert all subpackages recursively
        for collected_subpackage in packages.into_values() {
            let discovered_subpackage =
                self.convert_collected_to_discovered(py, collected_subpackage);
            discovered_package.add_direct_subpackage(discovered_subpackage);
        }

        discovered_package
    }
}
