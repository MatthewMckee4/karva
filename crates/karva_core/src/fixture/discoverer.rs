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

        tracing::info!("Discovering fixtures...");

        let walker = WalkBuilder::new(self.project.cwd().as_std_path())
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
                tracing::debug!("Discovering file: {}", path);
                let fixtures = fixture_definitions(&py, &path, self.project);
                for fixture in fixtures {
                    discovered_fixtures.insert(fixture);
                }
            }
        });

        DiscoveredFixtures::new(discovered_fixtures)
    }
}

#[derive(Debug)]
pub struct DiscoveredFixtures {
    pub fixtures: HashSet<Fixture>,
}

impl DiscoveredFixtures {
    #[must_use]
    pub const fn new(fixtures: HashSet<Fixture>) -> Self {
        Self { fixtures }
    }

    #[must_use]
    pub fn session_fixtures(&self, py: Python<'_>) -> HashMap<String, Py<PyAny>> {
        self.fixtures
            .iter()
            .filter(|fixture| fixture.scope == FixtureScope::Session)
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
    pub fn module_fixtures(&self, py: Python<'_>) -> HashMap<String, Py<PyAny>> {
        self.fixtures
            .iter()
            .filter(|fixture| fixture.scope == FixtureScope::Module)
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
    pub fn function_fixtures(&self, py: Python<'_>) -> HashMap<String, Py<PyAny>> {
        self.fixtures
            .iter()
            .filter(|fixture| fixture.scope == FixtureScope::Function)
            .filter_map(|fixture| match fixture.call(py) {
                Ok(fixture_return) => Some((fixture.name.clone(), fixture_return)),
                Err(e) => {
                    tracing::error!("Failed to call fixture {}: {}", fixture.name, e);
                    None
                }
            })
            .collect()
    }
}
