use karva_collector::{CollectedPackage, CollectionSettings, collect_file};

use std::collections::HashMap;

use camino::{Utf8Path, Utf8PathBuf};
use karva_project::path::TestPathFunction;

/// Collector for efficiently collecting specific test functions from test files.
///
/// Groups multiple test functions from the same file and collects them in a single parse,
/// improving performance when collecting many functions across the same files.
pub struct TestFunctionCollector<'a> {
    /// Current working directory.
    cwd: &'a Utf8Path,

    /// Configuration settings for test collection.
    settings: CollectionSettings<'a>,
}

impl<'a> TestFunctionCollector<'a> {
    pub fn new(cwd: &'a Utf8Path, settings: CollectionSettings<'a>) -> Self {
        Self { cwd, settings }
    }

    pub fn collect_all(&self, test_paths: Vec<TestPathFunction>) -> CollectedPackage {
        let mut session_package = CollectedPackage::new(self.cwd.to_path_buf());

        // Group test paths by file to avoid parsing the same file multiple times
        let mut file_to_functions: HashMap<Utf8PathBuf, Vec<String>> = HashMap::new();
        for test_path in test_paths {
            file_to_functions
                .entry(test_path.path.clone())
                .or_default()
                .push(test_path.function_name);
        }

        // Collect each file once with all its requested functions
        for (file_path, function_names) in file_to_functions {
            if let Some(module) =
                collect_file(&file_path, self.cwd, &self.settings, &function_names)
            {
                session_package.add_module(module);
            }

            self.collect_parent_configuration(&file_path, &mut session_package);
        }

        session_package.shrink();

        session_package
    }

    fn collect_parent_configuration(
        &self,
        path: &Utf8Path,
        session_package: &mut CollectedPackage,
    ) {
        let mut current_path = if path.is_dir() {
            path
        } else {
            match path.parent() {
                Some(parent) => parent,
                None => return,
            }
        };

        loop {
            let conftest_path = current_path.join("conftest.py");
            if conftest_path.exists() {
                let mut package = CollectedPackage::new(current_path.to_path_buf());

                if let Some(module) = collect_file(&conftest_path, self.cwd, &self.settings, &[]) {
                    package.add_configuration_module(module);
                    session_package.add_package(package);
                }
            }

            if current_path == self.cwd {
                break;
            }

            current_path = match current_path.parent() {
                Some(parent) => parent,
                None => break,
            };
        }
    }
}
