use indexmap::IndexMap;
use pyo3::{prelude::*, types::PyAny};

use crate::{
    diagnostic::Diagnostic,
    discovery::DiscoveredPackage,
    extensions::fixtures::{
        Finalizer, Finalizers, Fixture, FixtureScope, HasFixtures, UsesFixtures, builtins,
    },
    name::QualifiedFunctionName,
    utils::iter_with_ancestors,
};

/// Collection of fixtures and their finalizers for a specific scope.
#[derive(Debug, Default, Clone)]
pub(crate) struct FixtureCollection {
    /// Map of fixture names to their resolved Python values
    fixtures: IndexMap<QualifiedFunctionName, Vec<Py<PyAny>>>,
    /// List of cleanup functions to execute when this collection is reset
    finalizers: Vec<Finalizer>,
}

impl FixtureCollection {
    fn insert_fixture(
        &mut self,
        fixture_name: QualifiedFunctionName,
        fixture_return: Vec<Py<PyAny>>,
    ) {
        self.fixtures.insert(fixture_name, fixture_return);
    }

    fn insert_finalizer(&mut self, finalizer: Finalizer) {
        self.finalizers.push(finalizer);
    }

    /// Get a fixture by name
    ///
    /// If the fixture name matches a built-in fixture,
    /// it creates the fixture on-demand and stores it.
    fn get_fixture(
        &self,
        py: Python<'_>,
        fixture_name: &str,
        exclude: Option<&[&QualifiedFunctionName]>,
    ) -> Option<Vec<Py<PyAny>>> {
        if let Some((_, fixture)) = self.fixtures.iter().rev().find(|(name, _)| {
            name.function_name() == fixture_name
                && exclude.is_none_or(|exclude| !exclude.contains(name))
        }) {
            return Some(fixture.clone());
        }

        match fixture_name {
            _ if builtins::temp_path::is_temp_path_fixture_name(fixture_name) => {
                if let Some(path_obj) = builtins::temp_path::create_temp_dir(py) {
                    return Some(vec![path_obj]);
                }
            }
            _ => {}
        }

        None
    }

    fn reset(&mut self) -> Finalizers {
        self.fixtures.clear();
        Finalizers::new(self.finalizers.drain(..).collect())
    }

    #[cfg(test)]
    fn contains_fixture_with_name(&self, fixture_name: &str) -> bool {
        self.fixtures
            .iter()
            .any(|(name, _)| name.function_name() == fixture_name)
    }
}

/// Manages fixtures for a specific scope in the test execution hierarchy.
///
/// The `FixtureManager` follows a hierarchical structure where each manager
/// can have a parent, allowing fixture resolution to traverse up the scope
/// chain (function -> module -> package -> session). This enables proper
/// fixture inheritance and dependency resolution across different test scopes.
#[derive(Debug, Default, Clone)]
pub(crate) struct FixtureManager<'a> {
    /// Reference to the parent manager in the scope hierarchy
    parent: Option<&'a FixtureManager<'a>>,

    /// The actual fixtures and finalizers for this scope
    collection: FixtureCollection,

    /// The scope level this manager is responsible for
    scope: FixtureScope,

    /// The diagnostics from creating fixtures
    diagnostics: Vec<Diagnostic>,
}

impl<'a> FixtureManager<'a> {
    pub(crate) fn new(parent: Option<&'a Self>, scope: FixtureScope) -> Self {
        Self {
            parent,
            collection: FixtureCollection::default(),
            scope,
            diagnostics: Vec::new(),
        }
    }

    pub(crate) fn clear_diagnostics(&mut self) -> Vec<Diagnostic> {
        self.diagnostics.drain(..).collect()
    }

    #[cfg(test)]
    pub(crate) fn contains_fixture_with_name_at_scope(
        &self,
        fixture_name: &str,
        scope: &FixtureScope,
    ) -> bool {
        if self.scope == *scope {
            self.collection.contains_fixture_with_name(fixture_name)
        } else {
            self.parent
                .as_ref()
                .is_some_and(|p| p.contains_fixture_with_name_at_scope(fixture_name, scope))
        }
    }

    #[cfg(test)]
    pub(crate) fn contains_fixture_with_name(&self, fixture_name: &str) -> bool {
        if self.collection.contains_fixture_with_name(fixture_name) {
            return true;
        }
        self.parent
            .as_ref()
            .is_some_and(|parent| parent.contains_fixture_with_name(fixture_name))
    }

    pub(crate) fn has_fixture(&self, fixture_name: &QualifiedFunctionName) -> bool {
        if self.collection.fixtures.get(fixture_name).is_some() {
            return true;
        }
        self.parent
            .as_ref()
            .map_or_else(|| false, |parent| parent.has_fixture(fixture_name))
    }

    // Check if a fixture with the given name exists (including built-ins).
    //
    // We can optionally exclude specific function names from the search.
    pub(crate) fn get_fixture_with_name(
        &self,
        py: Python<'_>,
        fixture_name: &str,
        exclude: Option<&[&QualifiedFunctionName]>,
    ) -> Option<Vec<Py<PyAny>>> {
        // Check existing fixtures first in current scope
        if let Some(fixture) = self.collection.get_fixture(py, fixture_name, exclude) {
            return Some(fixture);
        }

        self.parent?
            .get_fixture_with_name(py, fixture_name, exclude)
    }

    pub(crate) fn insert_fixture(&mut self, fixture_return: Vec<Py<PyAny>>, fixture: &Fixture) {
        if self.scope <= *fixture.scope() {
            self.collection
                .insert_fixture(fixture.name().clone(), fixture_return);
        } else {
            // We should not reach this
        }
    }

    pub(crate) fn insert_finalizer(&mut self, finalizer: Finalizer, scope: &FixtureScope) {
        if self.scope <= *scope {
            self.collection.insert_finalizer(finalizer);
        } else {
            // We should not reach this
        }
    }

    /// Recursively resolves and executes fixture dependencies.
    ///
    /// This method ensures that all dependencies of a fixture are resolved and executed
    /// before the fixture itself is called. It performs a depth-first traversal of the
    /// dependency graph, checking both the current scope and parent scopes for required fixtures.
    fn ensure_fixture_dependencies<'proj>(
        &mut self,
        py: Python<'_>,
        parents: &[&'proj DiscoveredPackage],
        current: &'proj dyn HasFixtures<'proj>,
        fixture: &Fixture,
    ) {
        if self.has_fixture(fixture.name()) {
            // We have already called this fixture. So we can return.
            return;
        }

        // To ensure we can call the current fixture, we must first look at all of its dependencies,
        // and resolve them first.
        let current_dependencies = fixture.dependant_fixtures(py);

        // We need to get all of the fixtures in the current scope.
        let current_all_fixtures = current.all_fixtures(py, &[]);

        for dependency in &current_dependencies {
            let mut found = false;
            for dep_fixture in &current_all_fixtures {
                if dep_fixture.name().function_name() == dependency {
                    // Avoid infinite recursion by not processing the same fixture we're currently on
                    if dep_fixture.name() != fixture.name() {
                        self.ensure_fixture_dependencies(py, parents, current, dep_fixture);
                        found = true;
                        break;
                    }
                }
            }

            // We did not find the dependency in the current scope.
            // So we try the parent scopes.
            if !found {
                for (parent, parents_above_current_parent) in iter_with_ancestors(parents) {
                    let parent_fixture = (*parent).get_fixture(py, dependency);

                    if let Some(parent_fixture) = parent_fixture {
                        if parent_fixture.name() != fixture.name() {
                            self.ensure_fixture_dependencies(
                                py,
                                &parents_above_current_parent,
                                parent,
                                parent_fixture,
                            );
                            break;
                        }
                    }
                }
            }
        }

        let Some(module) = current.fixture_module() else {
            return;
        };

        match fixture.call(py, self, module) {
            Ok(fixture_return) => {
                self.insert_fixture(
                    fixture_return
                        .into_iter()
                        .map(pyo3::Bound::unbind)
                        .collect(),
                    fixture,
                );
            }
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
            }
        }
    }

    /// Add fixtures with the current scope to the fixture manager.
    ///
    /// This will ensure that all of the dependencies of the given fixtures are called first.
    pub(crate) fn add_fixtures<'proj>(
        &mut self,
        py: Python<'_>,
        parents: &[&'proj DiscoveredPackage],
        current: &'proj dyn HasFixtures<'proj>,
        scopes: &[FixtureScope],
        dependencies: &[&dyn UsesFixtures],
    ) {
        let fixtures = current.fixtures(py, scopes, dependencies);

        for fixture in fixtures {
            self.ensure_fixture_dependencies(py, parents, current, fixture);
        }
    }

    /// Clears all fixtures and returns finalizers for cleanup.
    ///
    /// This method is called when a scope ends to ensure proper cleanup
    /// of resources allocated by fixtures.
    pub(crate) fn reset_fixtures(&mut self) -> Finalizers {
        self.collection.reset()
    }
}

#[cfg(test)]
mod tests {
    use karva_project::project::Project;
    use karva_test::TestContext;

    use super::*;
    use crate::discovery::StandardDiscoverer;

    #[test]
    fn test_fixture_manager_add_fixtures_impl_one_dependency() {
        let env = TestContext::with_files([
            (
                "<test>/conftest.py",
                r"
import karva
@karva.fixture(scope='function')
def x():
    return 1
",
            ),
            ("<test>/test_1.py", "def test_1(x): pass"),
        ]);

        let tests_dir = env.mapped_path("<test>").unwrap();

        let test_path = tests_dir.join("test_1.py");

        let project = Project::new(env.cwd(), vec![env.cwd()]);

        let (session, _) = Python::attach(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(tests_dir).unwrap();

        let test_module = tests_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_function("test_1").unwrap();

        Python::attach(|py| {
            let mut manager = FixtureManager::new(None, FixtureScope::Function);

            manager.add_fixtures(
                py,
                &[],
                &tests_package,
                &[FixtureScope::Function],
                &[first_test_function],
            );

            assert!(manager.contains_fixture_with_name("x"));
        });
    }

    #[test]
    fn test_fixture_manager_add_fixtures_impl_two_dependencies() {
        let env = TestContext::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import karva
@karva.fixture(scope='function')
def x():
    return 2
",
            ),
            (
                "<test>/tests/inner/conftest.py",
                r"
import karva
@karva.fixture(scope='function')
def y(x):
    return 1
",
            ),
            ("<test>/tests/inner/test_1.py", "def test_1(y): pass"),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let tests_dir = mapped_dir.join("tests");
        let inner_dir = tests_dir.join("inner");
        let test_path = inner_dir.join("test_1.py");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::attach(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();

        let inner_package = tests_package.get_package(&inner_dir).unwrap();

        let test_module = inner_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_function("test_1").unwrap();

        Python::attach(|py| {
            let mut manager = FixtureManager::new(None, FixtureScope::Function);

            manager.add_fixtures(
                py,
                &[tests_package],
                inner_package,
                &[FixtureScope::Function],
                &[first_test_function],
            );

            assert!(manager.contains_fixture_with_name("x"));
            assert!(manager.contains_fixture_with_name("y"));
        });
    }

    #[test]
    fn test_fixture_manager_add_fixtures_impl_two_dependencies_in_parent() {
        let env = TestContext::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import karva
@karva.fixture(scope='function')
def x():
    return 2
@karva.fixture(scope='function')
def y(x):
    return 1
",
            ),
            ("<test>/tests/inner/test_1.py", "def test_1(y): pass"),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let tests_dir = mapped_dir.join("tests");
        let inner_dir = tests_dir.join("inner");
        let test_path = inner_dir.join("test_1.py");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::attach(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();

        let inner_package = tests_package.get_package(&inner_dir).unwrap();

        let test_module = inner_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_function("test_1").unwrap();

        Python::attach(|py| {
            let mut manager = FixtureManager::new(None, FixtureScope::Function);

            manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[FixtureScope::Function],
                &[first_test_function],
            );

            assert!(manager.contains_fixture_with_name("x"));
            assert!(manager.contains_fixture_with_name("y"));
        });
    }

    #[test]
    fn test_fixture_manager_add_fixtures_impl_three_dependencies() {
        let env = TestContext::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import karva
@karva.fixture(scope='function')
def x():
    return 2
",
            ),
            (
                "<test>/tests/inner/conftest.py",
                r"
import karva
@karva.fixture(scope='function')
def y(x):
    return 1
",
            ),
            (
                "<test>/tests/inner/inner/conftest.py",
                r"
import karva
@karva.fixture(scope='function')
def z(y):
    return 3
",
            ),
            ("<test>/tests/inner/inner/test_1.py", "def test_1(z): pass"),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let tests_dir = mapped_dir.join("tests");
        let inner_dir = tests_dir.join("inner");
        let inner_inner_dir = inner_dir.join("inner");
        let test_path = inner_inner_dir.join("test_1.py");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::attach(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();

        let inner_package = tests_package.get_package(&inner_dir).unwrap();

        let inner_inner_package = inner_package.get_package(&inner_inner_dir).unwrap();

        let test_module = inner_inner_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_function("test_1").unwrap();

        Python::attach(|py| {
            let mut manager = FixtureManager::new(None, FixtureScope::Function);

            manager.add_fixtures(
                py,
                &[tests_package, inner_package],
                inner_inner_package,
                &[FixtureScope::Function],
                &[first_test_function],
            );

            assert!(manager.contains_fixture_with_name("x"));
            assert!(manager.contains_fixture_with_name("y"));
            assert!(manager.contains_fixture_with_name("z"));
        });
    }

    #[test]
    fn test_fixture_manager_add_fixtures_impl_two_dependencies_different_scopes() {
        let env = TestContext::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import karva
@karva.fixture(scope='module')
def x():
    return 2
",
            ),
            (
                "<test>/tests/inner/conftest.py",
                r"
import karva
@karva.fixture(scope='function')
def y(x):
    return 1

@karva.fixture(scope='function')
def z(x):
    return 1
",
            ),
            ("<test>/tests/inner/test_1.py", "def test_1(y, z): pass"),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let tests_dir = mapped_dir.join("tests");
        let inner_dir = tests_dir.join("inner");
        let test_path = inner_dir.join("test_1.py");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::attach(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();

        let inner_package = tests_package.get_package(&inner_dir).unwrap();

        let test_module = inner_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_function("test_1").unwrap();

        Python::attach(|py| {
            let y_fixture = inner_package.get_fixture(py, "y").unwrap();
            let z_fixture = inner_package.get_fixture(py, "z").unwrap();

            let mut test_module_fixture_manager = FixtureManager::new(None, FixtureScope::Module);

            test_module_fixture_manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[FixtureScope::Module],
                &[y_fixture, z_fixture],
            );

            let mut function_fixture_manager =
                FixtureManager::new(Some(&test_module_fixture_manager), FixtureScope::Function);

            function_fixture_manager.add_fixtures(
                py,
                &[tests_package],
                inner_package,
                &[FixtureScope::Function],
                &[first_test_function],
            );

            assert!(test_module_fixture_manager.contains_fixture_with_name("x"));
            assert!(function_fixture_manager.contains_fixture_with_name("y"));
            assert!(function_fixture_manager.contains_fixture_with_name("z"));
        });
    }

    #[test]
    fn test_fixture_manager_add_fixtures_impl_three_dependencies_different_scopes() {
        let env = TestContext::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import karva
@karva.fixture(scope='session')
def x():
    return 2
",
            ),
            (
                "<test>/tests/inner/conftest.py",
                r"
import karva
@karva.fixture(scope='module')
def y(x):
    return 1
",
            ),
            (
                "<test>/tests/inner/inner/conftest.py",
                r"
import karva
@karva.fixture(scope='function')
def z(y):
    return 3
",
            ),
            ("<test>/tests/inner/inner/test_1.py", "def test_1(z): pass"),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let tests_dir = mapped_dir.join("tests");
        let inner_dir = tests_dir.join("inner");
        let inner_inner_dir = inner_dir.join("inner");
        let test_path = inner_inner_dir.join("test_1.py");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::attach(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();

        let inner_package = tests_package.get_package(&inner_dir).unwrap();

        let inner_inner_package = inner_package.get_package(&inner_inner_dir).unwrap();

        let test_module = inner_inner_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_function("test_1").unwrap();

        Python::attach(|py| {
            let y_fixture = inner_package.get_fixture(py, "y").unwrap();
            let z_fixture = inner_inner_package.get_fixture(py, "z").unwrap();

            let mut session_fixture_manager = FixtureManager::new(None, FixtureScope::Session);

            session_fixture_manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[FixtureScope::Session],
                &[y_fixture],
            );

            let mut module_fixture_manager =
                FixtureManager::new(Some(&session_fixture_manager), FixtureScope::Module);

            module_fixture_manager.add_fixtures(
                py,
                &[tests_package],
                inner_package,
                &[FixtureScope::Module],
                &[z_fixture],
            );

            let mut function_fixture_manager =
                FixtureManager::new(Some(&module_fixture_manager), FixtureScope::Function);

            function_fixture_manager.add_fixtures(
                py,
                &[tests_package, inner_package],
                inner_inner_package,
                &[FixtureScope::Function],
                &[first_test_function],
            );

            assert!(session_fixture_manager.contains_fixture_with_name("x"));
            assert!(module_fixture_manager.contains_fixture_with_name("y"));
            assert!(function_fixture_manager.contains_fixture_with_name("z"));
        });
    }

    #[test]
    fn test_fixture_manager_add_fixtures_impl_three_dependencies_different_scopes_with_fixture_in_function()
     {
        let env = TestContext::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import karva
@karva.fixture(scope='module')
def x():
    return 1

@karva.fixture(scope='function')
def y(x):
    return 1

@karva.fixture(scope='function')
def z(x, y):
    return 1
",
            ),
            ("<test>/tests/inner/test_1.py", "def test_1(z): pass"),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let tests_dir = mapped_dir.join("tests");
        let inner_dir = tests_dir.join("inner");
        let test_path = inner_dir.join("test_1.py");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::attach(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();

        let inner_package = tests_package.get_package(&inner_dir).unwrap();

        let test_module = inner_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_function("test_1").unwrap();

        Python::attach(|py| {
            let y_fixture = tests_package.get_fixture(py, "y").unwrap();
            let z_fixture = tests_package.get_fixture(py, "z").unwrap();

            let mut module_fixture_manager = FixtureManager::new(None, FixtureScope::Module);

            module_fixture_manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[FixtureScope::Module],
                &[y_fixture],
            );

            let mut function_fixture_manager =
                FixtureManager::new(Some(&module_fixture_manager), FixtureScope::Function);

            function_fixture_manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[FixtureScope::Function],
                &[y_fixture, z_fixture, first_test_function],
            );

            assert!(module_fixture_manager.contains_fixture_with_name("x"));
            assert!(function_fixture_manager.contains_fixture_with_name("y"));
            assert!(function_fixture_manager.contains_fixture_with_name("z"));
        });
    }

    #[test]
    fn test_fixture_manager_complex_nested_structure_with_session_fixtures() {
        let env = TestContext::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import karva
@karva.fixture(scope='session')
def database():
    return 'db_connection'
",
            ),
            (
                "<test>/tests/api/conftest.py",
                r"
import karva
@karva.fixture(scope='package')
def api_client(database):
    return 'api_client'
",
            ),
            (
                "<test>/tests/api/users/conftest.py",
                r"
import karva
@karva.fixture(scope='module')
def user(api_client):
    return 'test_user'
",
            ),
            (
                "<test>/tests/api/users/test_user_auth.py",
                r"
import karva
@karva.fixture(scope='function')
def auth_token(user):
    return 'token123'

def test_user_login(auth_token): pass",
            ),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let tests_dir = mapped_dir.join("tests");
        let api_dir = tests_dir.join("api");
        let users_dir = api_dir.join("users");
        let test_path = users_dir.join("test_user_auth.py");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::attach(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();
        let api_package = tests_package.get_package(&api_dir).unwrap();
        let users_package = api_package.get_package(&users_dir).unwrap();
        let test_module = users_package.get_module(&test_path).unwrap();

        let test_function = test_module.get_test_function("test_user_login").unwrap();

        Python::attach(|py| {
            let api_client_fixture = api_package.get_fixture(py, "api_client").unwrap();
            let user_fixture = users_package.get_fixture(py, "user").unwrap();
            let auth_token_fixture = test_module.get_fixture(py, "auth_token").unwrap();

            let mut session_fixture_manager = FixtureManager::new(None, FixtureScope::Session);

            session_fixture_manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[FixtureScope::Session],
                &[api_client_fixture],
            );

            let mut package_fixture_manager =
                FixtureManager::new(Some(&session_fixture_manager), FixtureScope::Package);

            package_fixture_manager.add_fixtures(
                py,
                &[tests_package],
                api_package,
                &[FixtureScope::Package],
                &[user_fixture],
            );

            let mut module_fixture_manager =
                FixtureManager::new(Some(&package_fixture_manager), FixtureScope::Module);

            module_fixture_manager.add_fixtures(
                py,
                &[],
                users_package,
                &[FixtureScope::Module],
                &[auth_token_fixture],
            );

            let mut function_fixture_manager =
                FixtureManager::new(Some(&module_fixture_manager), FixtureScope::Function);

            function_fixture_manager.add_fixtures(
                py,
                &[tests_package, api_package, users_package],
                test_module,
                &[FixtureScope::Function],
                &[test_function],
            );

            assert!(session_fixture_manager.contains_fixture_with_name("database"));
            assert!(package_fixture_manager.contains_fixture_with_name("api_client"));
            assert!(module_fixture_manager.contains_fixture_with_name("user"));
            assert!(function_fixture_manager.contains_fixture_with_name("auth_token"));
        });
    }

    #[test]
    fn test_fixture_manager_multiple_packages_same_level() {
        let env = TestContext::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import karva
@karva.fixture(scope='session')
def config():
    return {'env': 'test'}
",
            ),
            (
                "<test>/tests/package_a/conftest.py",
                r"
import karva
@karva.fixture(scope='package')
def service_a(config):
    return 'service_a'
",
            ),
            (
                "<test>/tests/package_b/conftest.py",
                r"
import karva
@karva.fixture(scope='package')
def service_b(config):
    return 'service_b'
",
            ),
            (
                "<test>/tests/package_a/test_a.py",
                "def test_a(service_a): pass",
            ),
            (
                "<test>/tests/package_b/test_b.py",
                "def test_b(service_b): pass",
            ),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let tests_dir = mapped_dir.join("tests");
        let package_a_dir = tests_dir.join("package_a");
        let package_b_dir = tests_dir.join("package_b");
        let test_a_path = package_a_dir.join("test_a.py");
        let test_b_path = package_b_dir.join("test_b.py");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::attach(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();
        let package_a = tests_package.get_package(&package_a_dir).unwrap();
        let package_b = tests_package.get_package(&package_b_dir).unwrap();

        let module_a = package_a.get_module(&test_a_path).unwrap();
        let module_b = package_b.get_module(&test_b_path).unwrap();

        let test_a = module_a.get_test_function("test_a").unwrap();
        let test_b = module_b.get_test_function("test_b").unwrap();

        Python::attach(|py| {
            let service_a_fixture = package_a.get_fixture(py, "service_a").unwrap();
            let service_b_fixture = package_b.get_fixture(py, "service_b").unwrap();

            let mut session_fixture_manager = FixtureManager::new(None, FixtureScope::Session);

            session_fixture_manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[FixtureScope::Session],
                &[service_a_fixture, service_b_fixture],
            );

            let mut package_fixture_manager =
                FixtureManager::new(Some(&session_fixture_manager), FixtureScope::Package);

            package_fixture_manager.add_fixtures(
                py,
                &[tests_package],
                package_a,
                &[FixtureScope::Package],
                &[test_a],
            );

            package_fixture_manager.add_fixtures(
                py,
                &[tests_package],
                package_b,
                &[FixtureScope::Package],
                &[test_b],
            );

            assert!(session_fixture_manager.contains_fixture_with_name("config"));
            assert!(package_fixture_manager.contains_fixture_with_name("service_a"));
            assert!(package_fixture_manager.contains_fixture_with_name("service_b"));
        });
    }

    #[test]
    fn test_fixture_manager_fixture_override_in_nested_packages() {
        let env = TestContext::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import karva
@karva.fixture(scope='function')
def data():
    return 'root_data'
",
            ),
            (
                "<test>/tests/child/conftest.py",
                r"
import karva
@karva.fixture(scope='function')
def data():
    return 'child_data'
",
            ),
            ("<test>/tests/test_root.py", "def test_root(data): pass"),
            (
                "<test>/tests/child/test_child.py",
                "def test_child(data): pass",
            ),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let tests_dir = mapped_dir.join("tests");
        let child_dir = tests_dir.join("child");
        let root_test_path = tests_dir.join("test_root.py");
        let child_test_path = child_dir.join("test_child.py");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::attach(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();
        let child_package = tests_package.get_package(&child_dir).unwrap();

        let root_module = tests_package.get_module(&root_test_path).unwrap();
        let child_module = child_package.get_module(&child_test_path).unwrap();

        let root_test = root_module.get_test_function("test_root").unwrap();
        let child_test = child_module.get_test_function("test_child").unwrap();

        Python::attach(|py| {
            let mut manager = FixtureManager::new(None, FixtureScope::Function);

            manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[FixtureScope::Function],
                &[root_test],
            );

            manager.reset_fixtures();
            manager.add_fixtures(
                py,
                &[tests_package],
                child_package,
                &[FixtureScope::Function],
                &[child_test],
            );

            assert!(manager.contains_fixture_with_name_at_scope("data", &FixtureScope::Function));
        });
    }

    #[test]
    fn test_fixture_manager_multiple_dependent_fixtures_same_scope() {
        let env = TestContext::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import karva
@karva.fixture(scope='function')
def base():
    return 'base'
@karva.fixture(scope='function')
def derived_a(base):
    return f'{base}_a'
@karva.fixture(scope='function')
def derived_b(base):
    return f'{base}_b'
@karva.fixture(scope='function')
def combined(derived_a, derived_b):
    return f'{derived_a}_{derived_b}'
",
            ),
            (
                "<test>/tests/test_combined.py",
                "def test_combined(combined): pass",
            ),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let tests_dir = mapped_dir.join("tests");
        let test_path = tests_dir.join("test_combined.py");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::attach(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();
        let test_module = tests_package.get_module(&test_path).unwrap();
        let test_function = test_module.get_test_function("test_combined").unwrap();

        Python::attach(|py| {
            let mut manager = FixtureManager::new(None, FixtureScope::Function);

            manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[FixtureScope::Function],
                &[test_function],
            );

            assert!(manager.contains_fixture_with_name_at_scope("base", &FixtureScope::Function));
            assert!(
                manager.contains_fixture_with_name_at_scope("derived_a", &FixtureScope::Function)
            );
            assert!(
                manager.contains_fixture_with_name_at_scope("derived_b", &FixtureScope::Function)
            );
            assert!(
                manager.contains_fixture_with_name_at_scope("combined", &FixtureScope::Function)
            );
        });
    }

    #[test]
    fn test_fixture_manager_deep_nesting_five_levels() {
        let env = TestContext::with_files([
            (
                "<test>/level1/conftest.py",
                r"
import karva
@karva.fixture(scope='session')
def level1():
    return 'l1'
",
            ),
            (
                "<test>/level1/level2/conftest.py",
                r"
import karva
@karva.fixture(scope='package')
def level2(level1):
    return 'l2'
",
            ),
            (
                "<test>/level1/level2/level3/conftest.py",
                r"
import karva
@karva.fixture(scope='module')
def level3(level2):
    return 'l3'
",
            ),
            (
                "<test>/level1/level2/level3/level4/conftest.py",
                r"
import karva
@karva.fixture(scope='function')
def level4(level3):
    return 'l4'
",
            ),
            (
                "<test>/level1/level2/level3/level4/level5/conftest.py",
                r"
import karva
@karva.fixture(scope='function')
def level5(level4):
    return 'l5'
",
            ),
            (
                "<test>/level1/level2/level3/level4/level5/test_deep.py",
                "def test_deep(level5): pass",
            ),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let l1_dir = mapped_dir.join("level1");
        let l2_dir = l1_dir.join("level2");
        let l3_dir = l2_dir.join("level3");
        let l4_dir = l3_dir.join("level4");
        let l5_dir = l4_dir.join("level5");
        let test_path = l5_dir.join("test_deep.py");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::attach(|py| StandardDiscoverer::new(&project).discover(py));

        let l1_package = session.get_package(&l1_dir).unwrap();
        let l2_package = l1_package.get_package(&l2_dir).unwrap();
        let l3_package = l2_package.get_package(&l3_dir).unwrap();
        let l4_package = l3_package.get_package(&l4_dir).unwrap();
        let l5_package = l4_package.get_package(&l5_dir).unwrap();

        let test_module = l5_package.get_module(&test_path).unwrap();
        let test_function = test_module.get_test_function("test_deep").unwrap();

        Python::attach(|py| {
            let l2_fixture = l2_package.get_fixture(py, "level2").unwrap();
            let l3_fixture = l3_package.get_fixture(py, "level3").unwrap();
            let l4_fixture = l4_package.get_fixture(py, "level4").unwrap();
            let l5_fixture = l5_package.get_fixture(py, "level5").unwrap();

            let mut session_fixture_manager = FixtureManager::new(None, FixtureScope::Session);

            session_fixture_manager.add_fixtures(
                py,
                &[],
                l1_package,
                &[FixtureScope::Session],
                &[l2_fixture],
            );
            let mut package_fixture_manager =
                FixtureManager::new(Some(&session_fixture_manager), FixtureScope::Package);

            package_fixture_manager.add_fixtures(
                py,
                &[l1_package],
                l2_package,
                &[FixtureScope::Package],
                &[l3_fixture],
            );

            let mut module_fixture_manager =
                FixtureManager::new(Some(&package_fixture_manager), FixtureScope::Module);

            module_fixture_manager.add_fixtures(
                py,
                &[l1_package, l2_package],
                l3_package,
                &[FixtureScope::Module],
                &[l4_fixture],
            );

            let mut function_fixture_manager =
                FixtureManager::new(Some(&module_fixture_manager), FixtureScope::Function);

            function_fixture_manager.add_fixtures(
                py,
                &[l1_package, l2_package, l3_package, l4_package],
                l5_package,
                &[FixtureScope::Function],
                &[l5_fixture, test_function],
            );

            assert!(session_fixture_manager.contains_fixture_with_name("level1"));
            assert!(package_fixture_manager.contains_fixture_with_name("level2"));
            assert!(module_fixture_manager.contains_fixture_with_name("level3"));
            assert!(function_fixture_manager.contains_fixture_with_name("level4"));
            assert!(function_fixture_manager.contains_fixture_with_name("level5"));
        });
    }

    #[test]
    fn test_fixture_manager_multiple_tests_same_module() {
        let env = TestContext::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import karva
@karva.fixture(scope='module')
def module_fixture():
    return 'module_data'

@karva.fixture(scope='function')
def function_fixture(module_fixture):
    return 'function_data'
",
            ),
            (
                "<test>/tests/test_multiple.py",
                "
def test_one(function_fixture): pass
def test_two(function_fixture): pass
def test_three(module_fixture): pass",
            ),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let tests_dir = mapped_dir.join("tests");
        let test_path = tests_dir.join("test_multiple.py");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::attach(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();
        let test_module = tests_package.get_module(&test_path).unwrap();

        let test_one = test_module.get_test_function("test_one").unwrap();
        let test_two = test_module.get_test_function("test_two").unwrap();
        let test_three = test_module.get_test_function("test_three").unwrap();

        Python::attach(|py| {
            let mut module_fixture_manager = FixtureManager::new(None, FixtureScope::Module);

            module_fixture_manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[FixtureScope::Module],
                &[test_one, test_two, test_three],
            );

            let mut function_fixture_manager =
                FixtureManager::new(Some(&module_fixture_manager), FixtureScope::Function);

            function_fixture_manager.add_fixtures(
                py,
                &[tests_package],
                tests_package,
                &[FixtureScope::Function],
                &[test_one, test_two, test_three],
            );

            assert!(module_fixture_manager.contains_fixture_with_name("module_fixture"));
            assert!(function_fixture_manager.contains_fixture_with_name("function_fixture"));
        });
    }

    #[test]
    fn test_fixture_manager_complex_dependency_chain_with_multiple_branches() {
        let env = TestContext::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import karva

@karva.fixture(scope='session')
def root():
    return 'root'

@karva.fixture(scope='package')
def branch_a1(root):
    return f'{root}_a1'

@karva.fixture(scope='module')
def branch_a2(branch_a1):
    return f'{branch_a1}_a2'

@karva.fixture(scope='package')
def branch_b1(root):
    return f'{root}_b1'

@karva.fixture(scope='module')
def branch_b2(branch_b1):
    return f'{branch_b1}_b2'

@karva.fixture(scope='function')
def converged(branch_a2, branch_b2):
    return f'{branch_a2}_{branch_b2}'
",
            ),
            (
                "<test>/tests/test_converged.py",
                "def test_converged(converged): pass",
            ),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let tests_dir = mapped_dir.join("tests");
        let test_path = tests_dir.join("test_converged.py");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::attach(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();
        let test_module = tests_package.get_module(&test_path).unwrap();
        let test_function = test_module.get_test_function("test_converged").unwrap();

        Python::attach(|py| {
            let branch_a1_fixture = tests_package.get_fixture(py, "branch_a1").unwrap();
            let branch_b1_fixture = tests_package.get_fixture(py, "branch_b1").unwrap();
            let branch_a2_fixture = tests_package.get_fixture(py, "branch_a2").unwrap();
            let branch_b2_fixture = tests_package.get_fixture(py, "branch_b2").unwrap();
            let converged_fixture = tests_package.get_fixture(py, "converged").unwrap();

            let mut session_fixture_manager = FixtureManager::new(None, FixtureScope::Session);

            session_fixture_manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[FixtureScope::Session],
                &[branch_a1_fixture, branch_b1_fixture],
            );

            let mut package_fixture_manager =
                FixtureManager::new(Some(&session_fixture_manager), FixtureScope::Package);

            package_fixture_manager.add_fixtures(
                py,
                &[tests_package],
                tests_package,
                &[FixtureScope::Package],
                &[branch_a2_fixture, branch_b2_fixture],
            );

            let mut module_fixture_manager =
                FixtureManager::new(Some(&package_fixture_manager), FixtureScope::Module);

            module_fixture_manager.add_fixtures(
                py,
                &[tests_package],
                tests_package,
                &[FixtureScope::Module],
                &[converged_fixture],
            );

            let mut function_fixture_manager =
                FixtureManager::new(Some(&module_fixture_manager), FixtureScope::Function);

            function_fixture_manager.add_fixtures(
                py,
                &[tests_package],
                tests_package,
                &[FixtureScope::Function],
                &[test_function],
            );

            assert!(session_fixture_manager.contains_fixture_with_name("root"));
            assert!(package_fixture_manager.contains_fixture_with_name("branch_a1"));
            assert!(package_fixture_manager.contains_fixture_with_name("branch_b1"));
            assert!(module_fixture_manager.contains_fixture_with_name("branch_a2"));
            assert!(module_fixture_manager.contains_fixture_with_name("branch_b2"));
            assert!(function_fixture_manager.contains_fixture_with_name("converged"));
        });
    }

    #[test]
    fn test_fixture_manager_reset_functions() {
        let env = TestContext::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import karva
@karva.fixture(scope='session')
def session_fixture():
    return 'session'

@karva.fixture(scope='package')
def package_fixture():
    return 'package'

@karva.fixture(scope='module')
def module_fixture():
    return 'module'

@karva.fixture(scope='function')
def function_fixture():
    return 'function'
",
            ),
            (
                "<test>/tests/test_reset.py",
                "def test_reset(session_fixture, package_fixture, module_fixture, function_fixture): pass",
            ),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let tests_dir = mapped_dir.join("tests");
        let test_path = tests_dir.join("test_reset.py");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::attach(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();

        let test_module = tests_package.get_module(&test_path).unwrap();

        let test_function = test_module.get_test_function("test_reset").unwrap();

        Python::attach(|py| {
            let mut session_fixture_manager = FixtureManager::new(None, FixtureScope::Session);

            session_fixture_manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[FixtureScope::Session],
                &[test_function],
            );

            let mut package_fixture_manager =
                FixtureManager::new(Some(&session_fixture_manager), FixtureScope::Package);

            package_fixture_manager.add_fixtures(
                py,
                &[tests_package],
                tests_package,
                &[FixtureScope::Package],
                &[test_function],
            );

            let mut module_fixture_manager =
                FixtureManager::new(Some(&package_fixture_manager), FixtureScope::Module);

            module_fixture_manager.add_fixtures(
                py,
                &[tests_package],
                tests_package,
                &[FixtureScope::Module],
                &[test_function],
            );

            let mut function_fixture_manager =
                FixtureManager::new(Some(&module_fixture_manager), FixtureScope::Function);

            function_fixture_manager.add_fixtures(
                py,
                &[tests_package],
                tests_package,
                &[FixtureScope::Function],
                &[test_function],
            );

            assert!(session_fixture_manager.contains_fixture_with_name("session_fixture"));
            assert!(package_fixture_manager.contains_fixture_with_name("package_fixture"));
            assert!(module_fixture_manager.contains_fixture_with_name("module_fixture"));
            assert!(function_fixture_manager.contains_fixture_with_name("function_fixture"));

            function_fixture_manager.reset_fixtures();
            assert!(
                !function_fixture_manager.contains_fixture_with_name_at_scope(
                    "function_fixture",
                    &FixtureScope::Function
                )
            );

            module_fixture_manager.reset_fixtures();
            assert!(
                !module_fixture_manager
                    .contains_fixture_with_name_at_scope("module_fixture", &FixtureScope::Module)
            );

            package_fixture_manager.reset_fixtures();
            assert!(
                !package_fixture_manager
                    .contains_fixture_with_name_at_scope("package_fixture", &FixtureScope::Package)
            );

            session_fixture_manager.reset_fixtures();
            assert!(
                !session_fixture_manager
                    .contains_fixture_with_name_at_scope("session_fixture", &FixtureScope::Session)
            );
        });
    }
}
