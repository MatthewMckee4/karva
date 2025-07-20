use std::collections::HashMap;

use pyo3::{prelude::*, types::PyAny};

use crate::{
    discovery::DiscoveredPackage,
    extensions::fixtures::{
        Finalizer, Finalizers, Fixture, FixtureScope, HasFixtures, RequiresFixtures,
    },
    utils::partition_iter,
};

#[derive(Debug, Default, Clone)]
pub(crate) struct FixtureCollection {
    fixtures: HashMap<String, Py<PyAny>>,
    finalizers: Vec<Finalizer>,
}

impl FixtureCollection {
    pub(crate) fn insert_fixture(&mut self, fixture_name: String, fixture_return: Py<PyAny>) {
        self.fixtures.insert(fixture_name, fixture_return);
    }

    pub(crate) fn insert_finalizer(&mut self, finalizer: Finalizer) {
        self.finalizers.push(finalizer);
    }

    pub(crate) fn iter_fixtures(&self) -> impl Iterator<Item = (&String, &Py<PyAny>)> {
        self.fixtures.iter()
    }

    pub(crate) fn reset(&mut self) -> Finalizers {
        self.fixtures.clear();
        Finalizers::new(self.finalizers.drain(..).collect())
    }

    pub(crate) fn contains_fixture(&self, fixture_name: &str) -> bool {
        self.fixtures.contains_key(fixture_name)
    }
}

// We use one [`FixtureManager`] for each scope.
#[derive(Debug, Default, Clone)]
pub(crate) struct FixtureManager<'a> {
    parent: Option<&'a FixtureManager<'a>>,
    collection: FixtureCollection,
    scope: FixtureScope,
}

impl<'a> FixtureManager<'a> {
    #[must_use]
    pub(crate) fn new(parent: Option<&'a FixtureManager<'a>>, scope: FixtureScope) -> Self {
        Self {
            parent,
            collection: FixtureCollection::default(),
            scope,
        }
    }

    #[must_use]
    pub(crate) fn scope(&self) -> &FixtureScope {
        &self.scope
    }

    #[must_use]
    pub(crate) fn contains_fixture_at_scope(
        &self,
        fixture_name: &str,
        scope: &FixtureScope,
    ) -> bool {
        if self.scope == *scope {
            self.collection.contains_fixture(fixture_name)
        } else {
            self.parent
                .as_ref()
                .map(|p| p.contains_fixture_at_scope(fixture_name, scope))
                .unwrap_or(false)
        }
    }

    pub(crate) fn contains_fixture(&self, fixture_name: &str) -> bool {
        self.all_fixtures().contains_key(fixture_name)
    }

    #[must_use]
    pub(crate) fn parent(&self) -> Option<&FixtureManager<'a>> {
        self.parent.as_ref()
    }

    #[must_use]
    pub(crate) fn get_fixture(&self, fixture_name: &str) -> Option<Py<PyAny>> {
        self.all_fixtures().get(fixture_name).cloned()
    }

    #[must_use]
    pub(crate) fn all_fixtures(&self) -> HashMap<String, Py<PyAny>> {
        let mut fixtures = HashMap::new();
        if let Some(parent) = &self.parent {
            fixtures.extend(parent.all_fixtures());
        }
        fixtures.extend(
            self.collection
                .iter_fixtures()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        fixtures
    }

    pub(crate) fn insert_fixture(&mut self, fixture_return: Py<PyAny>, fixture: &Fixture) {
        if self.scope == *fixture.scope() {
            self.collection
                .insert_fixture(fixture.name().to_string(), fixture_return);
        } else {
            if let Some(parent) = &mut self.parent {
                parent.insert_fixture(fixture_return, fixture);
            }
        }
    }

    pub(crate) fn insert_finalizer(&mut self, finalizer: Finalizer, scope: &FixtureScope) {
        if self.scope == *scope {
            self.collection.insert_finalizer(finalizer);
        } else {
            if let Some(parent) = &mut self.parent {
                parent.insert_finalizer(finalizer, scope);
            }
        }
    }

    // TODO: This is a bit of a mess.
    // This used to ensure that all of the given dependencies (fixtures) have been called.
    // This first starts with finding all dependencies of the given fixtures, and resolving and calling them first.
    //
    // We take the parents to ensure that if the dependent fixtures are not in the current scope,
    // we can still look for them in the parents.
    fn ensure_fixture_dependencies<'proj>(
        &mut self,
        py: Python<'_>,
        parents: &[&'proj DiscoveredPackage],
        current: &'proj dyn HasFixtures<'proj>,
        fixture: &Fixture,
    ) {
        if self.get_fixture(fixture.name()).is_some() {
            // We have already called this fixture. So we can just return.
            return;
        }

        // To ensure we can call the current fixture, we must first look at all of its dependencies,
        // and resolve them first.
        let current_dependencies = fixture.required_fixtures();

        // We need to get all of the fixtures in the current scope.
        let current_all_fixtures = current.all_fixtures(&[]);

        for dependency in &current_dependencies {
            let mut found = false;
            for fixture in &current_all_fixtures {
                if fixture.name() == dependency {
                    self.ensure_fixture_dependencies(py, parents, current, fixture);
                    found = true;
                    break;
                }
            }

            // We did not find the dependency in the current scope.
            // So we must try the parent scopes.
            if !found {
                for (parent, parents_above_current_parent) in partition_iter(parents) {
                    let parent_fixture = (*parent).get_fixture(dependency);

                    if let Some(parent_fixture) = parent_fixture {
                        self.ensure_fixture_dependencies(
                            py,
                            &parents_above_current_parent,
                            parent,
                            parent_fixture,
                        );
                    }
                    if self.contains_fixture(dependency) {
                        break;
                    }
                }
            }
        }

        match fixture.call(py, self) {
            Ok(fixture_return) => {
                self.insert_fixture(fixture_return.unbind(), fixture);
            }
            Err(e) => {
                tracing::debug!("Failed to call fixture {}: {}", fixture.name(), e);
            }
        }
    }

    pub(crate) fn add_fixtures<'proj>(
        &mut self,
        py: Python<'_>,
        parents: &[&'proj DiscoveredPackage],
        current: &'proj dyn HasFixtures<'proj>,
        scopes: &[FixtureScope],
        dependencies: &[&dyn RequiresFixtures],
    ) {
        let fixtures = current.fixtures(scopes, dependencies);

        for fixture in fixtures {
            self.ensure_fixture_dependencies(py, parents, current, fixture);
        }
    }

    pub(crate) fn reset_fixtures(&mut self) -> Finalizers {
        self.collection.reset()
    }
}

#[cfg(test)]
mod tests {
    use karva_project::{project::Project, testing::TestEnv};

    use super::*;
    use crate::discovery::StandardDiscoverer;

    #[test]
    fn test_fixture_manager_add_fixtures_impl_one_dependency() {
        let env = TestEnv::with_files([
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

        let (session, _) = Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(tests_dir).unwrap();

        let test_module = tests_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_function("test_1").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new(None, FixtureScope::Function);

            manager.add_fixtures(
                py,
                &[],
                &tests_package,
                &[FixtureScope::Function],
                &[first_test_function],
            );

            assert!(manager.contains_fixture("x"));
        });
    }

    #[test]
    fn test_fixture_manager_add_fixtures_impl_two_dependencies() {
        let env = TestEnv::with_files([
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
        let (session, _) = Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();

        let inner_package = tests_package.get_package(&inner_dir).unwrap();

        let test_module = inner_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_function("test_1").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new(None, FixtureScope::Function);

            manager.add_fixtures(
                py,
                &[tests_package],
                inner_package,
                &[FixtureScope::Function],
                &[first_test_function],
            );

            assert!(manager.contains_fixture("x"));
            assert!(manager.contains_fixture("y"));
        });
    }

    #[test]
    fn test_fixture_manager_add_fixtures_impl_two_dependencies_in_parent() {
        let env = TestEnv::with_files([
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
        let (session, _) = Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();

        let inner_package = tests_package.get_package(&inner_dir).unwrap();

        let test_module = inner_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_function("test_1").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new(None, FixtureScope::Function);

            manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[FixtureScope::Function],
                &[first_test_function],
            );

            assert!(manager.contains_fixture("x"));
            assert!(manager.contains_fixture("y"));
        });
    }

    #[test]
    fn test_fixture_manager_add_fixtures_impl_three_dependencies() {
        let env = TestEnv::with_files([
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
        let (session, _) = Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();

        let inner_package = tests_package.get_package(&inner_dir).unwrap();

        let inner_inner_package = inner_package.get_package(&inner_inner_dir).unwrap();

        let test_module = inner_inner_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_function("test_1").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new(None, FixtureScope::Function);

            manager.add_fixtures(
                py,
                &[tests_package, inner_package],
                inner_inner_package,
                &[FixtureScope::Function],
                &[first_test_function],
            );

            assert!(manager.contains_fixture("x"));
            assert!(manager.contains_fixture("y"));
            assert!(manager.contains_fixture("z"));
        });
    }

    #[test]
    fn test_fixture_manager_add_fixtures_impl_two_dependencies_different_scopes() {
        let env = TestEnv::with_files([
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
        let (session, _) = Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();

        let inner_package = tests_package.get_package(&inner_dir).unwrap();

        let test_module = inner_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_function("test_1").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new(None, FixtureScope::Function);

            manager.add_fixtures(
                py,
                &[tests_package],
                inner_package,
                &[FixtureScope::Function],
                &[first_test_function],
            );

            assert!(manager.contains_fixture_at_scope("x", &FixtureScope::Module));
            assert!(manager.contains_fixture_at_scope("y", &FixtureScope::Function));
            assert!(manager.contains_fixture_at_scope("z", &FixtureScope::Function));
        });
    }

    #[test]
    fn test_fixture_manager_add_fixtures_impl_three_dependencies_different_scopes() {
        let env = TestEnv::with_files([
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
        let (session, _) = Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();

        let inner_package = tests_package.get_package(&inner_dir).unwrap();

        let inner_inner_package = inner_package.get_package(&inner_inner_dir).unwrap();

        let test_module = inner_inner_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_function("test_1").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new(None, FixtureScope::Function);

            manager.add_fixtures(
                py,
                &[tests_package, inner_package],
                inner_inner_package,
                &[FixtureScope::Function],
                &[first_test_function],
            );

            assert!(manager.contains_fixture_at_scope("x", &FixtureScope::Session));
            assert!(manager.contains_fixture_at_scope("y", &FixtureScope::Module));
            assert!(manager.contains_fixture_at_scope("z", &FixtureScope::Function));
        });
    }

    #[test]
    fn test_fixture_manager_add_fixtures_impl_three_dependencies_different_scopes_with_fixture_in_function()
     {
        let env = TestEnv::with_files([
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
        let (session, _) = Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();

        let inner_package = tests_package.get_package(&inner_dir).unwrap();

        let test_module = inner_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_function("test_1").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new(None, FixtureScope::Function);

            manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[FixtureScope::Function, FixtureScope::Module],
                &[first_test_function],
            );

            assert!(manager.contains_fixture_at_scope("x", &FixtureScope::Module));
            assert!(manager.contains_fixture_at_scope("y", &FixtureScope::Function));
            assert!(manager.contains_fixture_at_scope("z", &FixtureScope::Function));
        });
    }

    #[test]
    fn test_fixture_manager_complex_nested_structure_with_session_fixtures() {
        let env = TestEnv::with_files([
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
        let (session, _) = Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();
        let api_package = tests_package.get_package(&api_dir).unwrap();
        let users_package = api_package.get_package(&users_dir).unwrap();
        let test_module = users_package.get_module(&test_path).unwrap();
        let test_function = test_module.get_test_function("test_user_login").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new(None, FixtureScope::Function);

            manager.add_fixtures(
                py,
                &[tests_package, api_package, users_package],
                test_module,
                &[
                    FixtureScope::Session,
                    FixtureScope::Package,
                    FixtureScope::Module,
                    FixtureScope::Function,
                ],
                &[test_function],
            );

            assert!(manager.contains_fixture_at_scope("api_client", &FixtureScope::Package));
            assert!(manager.contains_fixture_at_scope("user", &FixtureScope::Module));
            assert!(manager.contains_fixture_at_scope("auth_token", &FixtureScope::Function));
            assert!(manager.contains_fixture_at_scope("database", &FixtureScope::Session));
        });
    }

    #[test]
    fn test_fixture_manager_multiple_packages_same_level() {
        let env = TestEnv::with_files([
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
        let (session, _) = Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();
        let package_a = tests_package.get_package(&package_a_dir).unwrap();
        let package_b = tests_package.get_package(&package_b_dir).unwrap();

        let module_a = package_a.get_module(&test_a_path).unwrap();
        let module_b = package_b.get_module(&test_b_path).unwrap();

        let test_a = module_a.get_test_function("test_a").unwrap();
        let test_b = module_b.get_test_function("test_b").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new(None, FixtureScope::Function);

            manager.add_fixtures(
                py,
                &[tests_package],
                package_a,
                &[FixtureScope::Session, FixtureScope::Package],
                &[test_a],
            );

            assert!(manager.contains_fixture_at_scope("config", &FixtureScope::Session));
            assert!(manager.contains_fixture_at_scope("service_a", &FixtureScope::Package));

            manager.reset_fixtures();

            manager.add_fixtures(
                py,
                &[tests_package],
                package_b,
                &[FixtureScope::Session, FixtureScope::Package],
                &[test_b],
            );

            assert!(manager.contains_fixture_at_scope("config", &FixtureScope::Session));
            assert!(manager.contains_fixture_at_scope("service_b", &FixtureScope::Package));
            assert!(!manager.contains_fixture_at_scope("service_a", &FixtureScope::Package));
        });
    }

    #[test]
    fn test_fixture_manager_fixture_override_in_nested_packages() {
        let env = TestEnv::with_files([
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
        let (session, _) = Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();
        let child_package = tests_package.get_package(&child_dir).unwrap();

        let root_module = tests_package.get_module(&root_test_path).unwrap();
        let child_module = child_package.get_module(&child_test_path).unwrap();

        let root_test = root_module.get_test_function("test_root").unwrap();
        let child_test = child_module.get_test_function("test_child").unwrap();

        Python::with_gil(|py| {
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

            assert!(manager.contains_fixture_at_scope("data", &FixtureScope::Function));
        });
    }

    #[test]
    fn test_fixture_manager_multiple_dependent_fixtures_same_scope() {
        let env = TestEnv::with_files([
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
        let (session, _) = Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();
        let test_module = tests_package.get_module(&test_path).unwrap();
        let test_function = test_module.get_test_function("test_combined").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new(None, FixtureScope::Function);

            manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[FixtureScope::Function],
                &[test_function],
            );

            assert!(manager.contains_fixture_at_scope("base", &FixtureScope::Function));
            assert!(manager.contains_fixture_at_scope("derived_a", &FixtureScope::Function));
            assert!(manager.contains_fixture_at_scope("derived_b", &FixtureScope::Function));
            assert!(manager.contains_fixture_at_scope("combined", &FixtureScope::Function));
        });
    }

    #[test]
    fn test_fixture_manager_deep_nesting_five_levels() {
        let env = TestEnv::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import karva
@karva.fixture(scope='session')
def level1():
    return 'l1'
",
            ),
            (
                "<test>/tests/level2/conftest.py",
                r"
import karva
@karva.fixture(scope='package')
def level2(level1):
    return 'l2'
",
            ),
            (
                "<test>/tests/level2/level3/conftest.py",
                r"
import karva
@karva.fixture(scope='module')
def level3(level2):
    return 'l3'
",
            ),
            (
                "<test>/tests/level2/level3/level4/conftest.py",
                r"
import karva
@karva.fixture(scope='function')
def level4(level3):
    return 'l4'
",
            ),
            (
                "<test>/tests/level2/level3/level4/level5/conftest.py",
                r"
import karva
@karva.fixture(scope='function')
def level5(level4):
    return 'l5'
",
            ),
            (
                "<test>/tests/level2/level3/level4/level5/test_deep.py",
                "def test_deep(level5): pass",
            ),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let tests_dir = mapped_dir.join("tests");
        let l2_dir = tests_dir.join("level2");
        let l3_dir = l2_dir.join("level3");
        let l4_dir = l3_dir.join("level4");
        let l5_dir = l4_dir.join("level5");
        let test_path = l5_dir.join("test_deep.py");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

        let l1_package = session.get_package(&tests_dir).unwrap();
        let l2_package = l1_package.get_package(&l2_dir).unwrap();
        let l3_package = l2_package.get_package(&l3_dir).unwrap();
        let l4_package = l3_package.get_package(&l4_dir).unwrap();
        let l5_package = l4_package.get_package(&l5_dir).unwrap();

        let test_module = l5_package.get_module(&test_path).unwrap();
        let test_function = test_module.get_test_function("test_deep").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new(None, FixtureScope::Function);

            manager.add_fixtures(
                py,
                &[l1_package, l2_package, l3_package, l4_package],
                l5_package,
                &[
                    FixtureScope::Session,
                    FixtureScope::Package,
                    FixtureScope::Module,
                    FixtureScope::Function,
                ],
                &[test_function],
            );

            assert!(manager.contains_fixture_at_scope("level1", &FixtureScope::Session));
            assert!(manager.contains_fixture_at_scope("level2", &FixtureScope::Package));
            assert!(manager.contains_fixture_at_scope("level3", &FixtureScope::Module));
            assert!(manager.contains_fixture_at_scope("level4", &FixtureScope::Function));
            assert!(manager.contains_fixture_at_scope("level5", &FixtureScope::Function));
        });
    }

    #[test]
    fn test_fixture_manager_cross_package_dependencies() {
        let env = TestEnv::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import karva
@karva.fixture(scope='session')
def utils():
    return 'shared_utils'
",
            ),
            (
                "<test>/tests/package_a/conftest.py",
                r"
import karva
@karva.fixture(scope='package')
def service_a(utils):
    return f'service_a_{utils}'
",
            ),
            (
                "<test>/tests/package_b/conftest.py",
                r"
import karva
@karva.fixture(scope='package')
def service_b(utils):
    return f'service_b_{utils}'
",
            ),
            (
                "<test>/tests/package_c/conftest.py",
                r"
import karva
@karva.fixture(scope='function')
def integration_service(service_a, service_b):
    return f'integration_{service_a}_{service_b}'
",
            ),
            (
                "<test>/tests/package_c/test_integration.py",
                "def test_integration(integration_service): pass",
            ),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let tests_dir = mapped_dir.join("tests");
        let package_a_dir = tests_dir.join("package_a");
        let package_b_dir = tests_dir.join("package_b");
        let package_c_dir = tests_dir.join("package_c");
        let test_path = package_c_dir.join("test_integration.py");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();
        let package_a = tests_package.get_package(&package_a_dir).unwrap();
        let package_b = tests_package.get_package(&package_b_dir).unwrap();
        let package_c = tests_package.get_package(&package_c_dir).unwrap();

        let test_module = package_c.get_module(&test_path).unwrap();
        let test_function = test_module.get_test_function("test_integration").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new(None, FixtureScope::Function);

            manager.add_fixtures(
                py,
                &[tests_package],
                package_a,
                &[FixtureScope::Session, FixtureScope::Package],
                &[],
            );

            manager.add_fixtures(
                py,
                &[tests_package],
                package_b,
                &[FixtureScope::Session, FixtureScope::Package],
                &[],
            );

            manager.add_fixtures(
                py,
                &[tests_package],
                package_c,
                &[
                    FixtureScope::Session,
                    FixtureScope::Package,
                    FixtureScope::Function,
                ],
                &[test_function],
            );

            assert!(manager.contains_fixture_at_scope("utils", &FixtureScope::Session));
            assert!(manager.contains_fixture_at_scope("service_a", &FixtureScope::Package));
            assert!(manager.contains_fixture_at_scope("service_b", &FixtureScope::Package));
            assert!(
                manager.contains_fixture_at_scope("integration_service", &FixtureScope::Function)
            );
        });
    }

    #[test]
    fn test_fixture_manager_multiple_tests_same_module() {
        let env = TestEnv::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import karva
@karva.fixture(scope='module')
def module_fixture():
    return 'module_data'
import karva
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
        let (session, _) = Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();
        let test_module = tests_package.get_module(&test_path).unwrap();

        let test_one = test_module.get_test_function("test_one").unwrap();
        let test_two = test_module.get_test_function("test_two").unwrap();
        let test_three = test_module.get_test_function("test_three").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new(None, FixtureScope::Function);

            manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[FixtureScope::Module, FixtureScope::Function],
                &[test_one, test_two, test_three],
            );

            assert!(manager.contains_fixture_at_scope("module_fixture", &FixtureScope::Module));
            assert!(manager.contains_fixture_at_scope("function_fixture", &FixtureScope::Function));
        });
    }

    #[test]
    fn test_fixture_manager_complex_dependency_chain_with_multiple_branches() {
        let env = TestEnv::with_files([
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
        let (session, _) = Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();
        let test_module = tests_package.get_module(&test_path).unwrap();
        let test_function = test_module.get_test_function("test_converged").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new(None, FixtureScope::Function);

            manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[
                    FixtureScope::Session,
                    FixtureScope::Package,
                    FixtureScope::Module,
                    FixtureScope::Function,
                ],
                &[test_function],
            );

            assert!(manager.contains_fixture_at_scope("root", &FixtureScope::Session));
            assert!(manager.contains_fixture_at_scope("branch_a1", &FixtureScope::Package));
            assert!(manager.contains_fixture_at_scope("branch_b1", &FixtureScope::Package));
            assert!(manager.contains_fixture_at_scope("branch_a2", &FixtureScope::Module));
            assert!(manager.contains_fixture_at_scope("branch_b2", &FixtureScope::Module));
            assert!(manager.contains_fixture_at_scope("converged", &FixtureScope::Function));
        });
    }

    #[test]
    fn test_fixture_manager_reset_functions() {
        let env = TestEnv::with_files([
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
        let (session, _) = Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();

        let test_module = tests_package.get_module(&test_path).unwrap();

        let test_function = test_module.get_test_function("test_reset").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new(None, FixtureScope::Function);

            manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[
                    FixtureScope::Session,
                    FixtureScope::Package,
                    FixtureScope::Module,
                    FixtureScope::Function,
                ],
                &[test_function],
            );

            assert!(manager.contains_fixture_at_scope("session_fixture", &FixtureScope::Session));
            assert!(manager.contains_fixture_at_scope("package_fixture", &FixtureScope::Package));
            assert!(manager.contains_fixture_at_scope("module_fixture", &FixtureScope::Module));
            assert!(manager.contains_fixture_at_scope("function_fixture", &FixtureScope::Function));

            manager.reset_fixtures();
            assert!(
                !manager.contains_fixture_at_scope("function_fixture", &FixtureScope::Function)
            );
            assert!(manager.contains_fixture_at_scope("module_fixture", &FixtureScope::Module));

            manager.reset_fixtures();
            assert!(!manager.contains_fixture_at_scope("module_fixture", &FixtureScope::Module));
            assert!(manager.contains_fixture_at_scope("package_fixture", &FixtureScope::Package));

            manager.reset_fixtures();
            assert!(!manager.contains_fixture_at_scope("package_fixture", &FixtureScope::Package));
            assert!(manager.contains_fixture_at_scope("session_fixture", &FixtureScope::Session));

            manager.reset_fixtures();
            assert!(!manager.contains_fixture_at_scope("session_fixture", &FixtureScope::Session));
        });
    }
}
