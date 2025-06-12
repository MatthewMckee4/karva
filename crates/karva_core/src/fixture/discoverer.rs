use std::collections::{HashMap, HashSet};

use ignore::WalkBuilder;
use karva_project::{path::SystemPathBuf, project::Project, utils::is_python_file};
use pyo3::prelude::*;

use crate::fixture::{Fixture, FixtureScope, visitor::fixture_definitions};

pub struct FixtureDiscoverer<'proj> {
    project: &'proj Project,
}

impl<'proj> FixtureDiscoverer<'proj> {
    #[must_use]
    pub const fn new(project: &'proj Project) -> Self {
        Self { project }
    }

    #[must_use]
    pub fn discover(self) -> DiscoveredFixtures {
        let mut discovered_fixtures: HashSet<Fixture> = HashSet::new();

        let parent_test_path = self.project.parent_test_path();

        tracing::info!("Discovering fixtures in {}", parent_test_path);

        let walker = WalkBuilder::new(parent_test_path.as_std_path())
            .standard_filters(true)
            .require_git(false)
            .parents(false)
            .build();

        Python::with_gil(|py| {
            for entry in walker.flatten() {
                let entry_path = entry.path();
                let path = SystemPathBuf::from(entry_path);

                if !is_python_file(&path) {
                    tracing::debug!("Skipping non-python file: {}", path);
                    continue;
                }

                tracing::debug!("Discovering fixtures in file: {}", path);
                let fixtures = fixture_definitions(&py, &path, self.project);
                for fixture in fixtures {
                    discovered_fixtures.insert(fixture);
                }
            }
        });

        DiscoveredFixtures::new(discovered_fixtures)
    }

    fn discover_file(&self, py: &Python<'_>, path: &SystemPathBuf) -> Vec<Fixture> {
        fixture_definitions(py, &path, self.project)
    }

    fn discover_directory(
        &self,
        path: &SystemPathBuf,
        all_discovered_tests: &mut HashMap<Module<'proj>, IndexSet<TestCase>>,
    ) {
        let dir_path = path.as_std_path().to_path_buf();

        let walker = WalkBuilder::new(self.project.cwd().as_std_path())
            .standard_filters(true)
            .require_git(false)
            .parents(false)
            .filter_entry(move |entry| entry.path().starts_with(&dir_path))
            .build();

        for entry in walker.flatten() {
            let entry_path = entry.path();
            let path = SystemPathBuf::from(entry_path);

            if !is_python_file(&path) {
                tracing::debug!("Skipping non-python file: {}", entry.path().display());
                continue;
            }
            tracing::debug!("Discovering file: {}", entry.path().display());
            let test_cases = self.discover_file(&path);
            if !test_cases.is_empty() {
                all_discovered_tests.insert(Module::new(&path, self.project), test_cases);
            }
        }
    }
}

#[derive(Debug)]
pub struct DiscoveredFixtures {
    pub session: HashMap<String, Fixture>,
    pub package: HashMap<String, Fixture>,
    pub module: HashMap<String, Fixture>,
    pub function: HashMap<String, Fixture>,
}

impl DiscoveredFixtures {
    #[must_use]
    pub const fn new(fixtures: HashSet<Fixture>) -> Self {
        Self { fixtures }
    }

    #[must_use]
    fn get_fixtures_by_scope(
        &self,
        py: Python<'_>,
        scope: &FixtureScope,
    ) -> HashMap<String, Py<PyAny>> {
        self.fixtures
            .iter()
            .filter(|fixture| fixture.scope == *scope)
            .filter_map(|fixture| match fixture.call(py) {
                Ok(fixture_return) => Some((fixture.name.clone(), fixture_return)),
                Err(e) => {
                    tracing::error!("Failed to call fixture {}: {}", fixture.name, e);
                    None
                }
            })
            .collect()
    }

    #[must_use]
    pub fn session_fixtures(&self, py: Python<'_>) -> HashMap<String, Py<PyAny>> {
        self.get_fixtures_by_scope(py, &FixtureScope::Session)
    }

    #[must_use]
    pub fn module_fixtures(&self, py: Python<'_>) -> HashMap<String, Py<PyAny>> {
        self.get_fixtures_by_scope(py, &FixtureScope::Module)
    }

    #[must_use]
    pub fn function_fixtures(&self, py: Python<'_>) -> HashMap<String, Py<PyAny>> {
        self.get_fixtures_by_scope(py, &FixtureScope::Function)
    }
}
