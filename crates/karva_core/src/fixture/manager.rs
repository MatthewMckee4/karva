use std::collections::HashMap;

use pyo3::{prelude::*, types::PyAny};

use crate::{
    case::TestCase,
    fixture::{Fixture, FixtureRequester, FixtureScope, HasFixtures},
};

#[derive(Debug, Default)]
pub struct FixtureManager<'proj> {
    session_fixtures: HashMap<String, Bound<'proj, PyAny>>,
    module_fixtures: HashMap<String, Bound<'proj, PyAny>>,
    package_fixtures: HashMap<String, Bound<'proj, PyAny>>,
    function_fixtures: HashMap<String, Bound<'proj, PyAny>>,
}

impl<'proj> FixtureManager<'proj> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            session_fixtures: HashMap::new(),
            module_fixtures: HashMap::new(),
            package_fixtures: HashMap::new(),
            function_fixtures: HashMap::new(),
        }
    }

    #[must_use]
    pub fn get_fixture(&self, fixture_name: &str) -> Option<Bound<'proj, PyAny>> {
        self.all_fixtures().get(fixture_name).cloned()
    }

    #[must_use]
    pub fn all_fixtures(&self) -> HashMap<String, Bound<'proj, PyAny>> {
        let mut fixtures = HashMap::new();
        fixtures.extend(
            self.session_fixtures
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        fixtures.extend(
            self.module_fixtures
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        fixtures.extend(
            self.package_fixtures
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        fixtures.extend(
            self.function_fixtures
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        fixtures
    }

    fn add_fixtures_impl(
        &self,
        py: Python<'proj>,
        has_fixtures: &'proj (dyn HasFixtures<'proj> + 'proj),
        scope: &[FixtureScope],
        test_cases: &[&TestCase],
    ) -> HashMap<String, Bound<'proj, PyAny>> {
        let fixtures = has_fixtures.fixtures(scope, Some(test_cases));
        let mut called_fixtures: HashMap<String, Bound<'proj, PyAny>> = HashMap::new();
        let mut pending_fixtures: Vec<&Fixture> = fixtures.into_iter().collect();

        let prev_pending_len = pending_fixtures.len();

        while !pending_fixtures.is_empty() {
            let mut remaining_fixtures = Vec::new();

            for fixture in pending_fixtures {
                let required_fixtures_names = fixture.get_required_fixture_names();
                let mut required_fixtures = Vec::new();
                let mut all_dependencies_available = true;

                for name in required_fixtures_names {
                    if let Some(fixture) = self.get_fixture(&name) {
                        required_fixtures.push(fixture);
                    } else if let Some(fixture) = called_fixtures.get(&name) {
                        required_fixtures.push(fixture.clone());
                    } else if let Some(fixture_def) = has_fixtures.get_fixture(&name) {
                        remaining_fixtures.push(fixture_def);
                        all_dependencies_available = false;
                        break;
                    } else {
                        all_dependencies_available = false;
                        break;
                    }
                }

                if all_dependencies_available {
                    match fixture.call(py, required_fixtures) {
                        Ok(fixture_return) => {
                            called_fixtures.insert(fixture.name().to_string(), fixture_return);
                        }
                        Err(e) => {
                            tracing::error!("Failed to call fixture {}: {}", fixture.name(), e);
                        }
                    }
                } else {
                    remaining_fixtures.push(fixture);
                }
            }

            if remaining_fixtures.len() == prev_pending_len {
                // No progress made in this iteration, we have a circular dependency
                for fixture in &remaining_fixtures {
                    tracing::error!(
                        "Circular dependency detected for fixture {}",
                        fixture.name()
                    );
                }
                break;
            }

            pending_fixtures = remaining_fixtures;
        }

        called_fixtures
    }

    pub fn add_session_fixtures(
        &mut self,
        py: Python<'proj>,
        has_fixtures: &'proj dyn HasFixtures<'proj>,
        scope: &[FixtureScope],
        test_cases: &[&TestCase],
    ) {
        let called_fixtures = self.add_fixtures_impl(py, has_fixtures, scope, test_cases);
        self.session_fixtures.extend(called_fixtures);
    }

    pub fn reset_session_fixtures(&mut self) {
        self.session_fixtures.clear();
    }

    pub fn add_package_fixtures(
        &mut self,
        py: Python<'proj>,
        has_fixtures: &'proj dyn HasFixtures<'proj>,
        scope: &[FixtureScope],
        test_cases: &[&TestCase],
    ) {
        let called_fixtures = self.add_fixtures_impl(py, has_fixtures, scope, test_cases);
        self.package_fixtures.extend(called_fixtures);
    }

    pub fn reset_package_fixtures(&mut self) {
        self.package_fixtures.clear();
    }

    pub fn add_module_fixtures(
        &mut self,
        py: Python<'proj>,
        has_fixtures: &'proj dyn HasFixtures<'proj>,
        scope: &[FixtureScope],
        test_cases: &[&TestCase],
    ) {
        let called_fixtures = self.add_fixtures_impl(py, has_fixtures, scope, test_cases);
        self.module_fixtures.extend(called_fixtures);
    }

    pub fn reset_module_fixtures(&mut self) {
        self.module_fixtures.clear();
    }

    pub fn add_function_fixtures(
        &mut self,
        py: Python<'proj>,
        has_fixtures: &'proj dyn HasFixtures<'proj>,
        scope: &[FixtureScope],
        test_cases: &[&TestCase],
    ) {
        let called_fixtures = self.add_fixtures_impl(py, has_fixtures, scope, test_cases);
        self.function_fixtures.extend(called_fixtures);
    }

    pub fn reset_function_fixtures(&mut self) {
        self.function_fixtures.clear();
    }
}
