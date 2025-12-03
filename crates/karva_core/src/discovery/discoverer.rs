use camino::{Utf8Path, Utf8PathBuf};
use ignore::WalkBuilder;
use karva_project::TestPath;
use pyo3::prelude::*;

#[cfg(test)]
use crate::utils::attach_with_project;
use crate::{
    Context,
    diagnostic::report_invalid_path,
    discovery::{DiscoveredModule, DiscoveredPackage, ModuleType, visitor::DiscoveredFunctions},
    name::ModulePath,
    utils::add_to_sys_path,
};

pub struct StandardDiscoverer<'ctx, 'proj, 'rep> {
    context: &'ctx Context<'proj, 'rep>,
}

impl<'ctx, 'proj, 'rep> StandardDiscoverer<'ctx, 'proj, 'rep> {
    pub const fn new(context: &'ctx Context<'proj, 'rep>) -> Self {
        Self { context }
    }

    #[cfg(test)]
    pub(crate) fn discover(self) -> DiscoveredPackage {
        attach_with_project(self.context.project(), |py| self.discover_with_py(py))
    }

    pub(crate) fn discover_with_py(self, py: Python<'_>) -> DiscoveredPackage {
        let mut session_package = DiscoveredPackage::new(self.context.project().cwd().clone());

        let cwd = self.context.project().cwd();

        if add_to_sys_path(py, cwd, 0).is_err() {
            return session_package;
        }

        tracing::info!("Discovering tests...");

        for path in self.context.project().test_paths() {
            match path {
                Ok(path) => {
                    match &path {
                        TestPath::File(path) => {
                            let Some(module) =
                                self.discover_test_file(py, path, DiscoveryMode::All)
                            else {
                                continue;
                            };

                            session_package.add_module(module);
                        }
                        TestPath::Directory(path) => {
                            let package = self.discover_directory(py, path, DiscoveryMode::All);

                            session_package.add_package(package);
                        }
                        TestPath::Function {
                            path,
                            function_name,
                        } => {
                            let Some(mut module) =
                                self.discover_test_file(py, path, DiscoveryMode::All)
                            else {
                                continue;
                            };

                            module.filter_test_functions(function_name);

                            if !module.test_functions().is_empty() {
                                session_package.add_module(module);
                            }
                        }
                    }

                    self.add_parent_configuration_packages(py, path.path(), &mut session_package);
                }
                Err(error) => {
                    report_invalid_path(self.context, &error);
                }
            }
        }

        session_package.shrink();

        session_package
    }

    // Parse and run discovery on a single file
    fn discover_test_file(
        &self,
        py: Python,
        path: &Utf8PathBuf,
        discovery_mode: DiscoveryMode,
    ) -> Option<DiscoveredModule> {
        let module_path = ModulePath::new(path, self.context.project().cwd())?;

        let mut module = DiscoveredModule::new(module_path, path.into());

        let DiscoveredFunctions {
            functions,
            fixtures,
        } = super::visitor::discover(self.context, py, &module);

        if !discovery_mode.is_configuration_only() {
            module.extend_test_functions(functions);
        }

        module.extend_fixtures(fixtures);

        Some(module)
    }

    // This should look from the parent of path to the cwd for configuration files
    fn add_parent_configuration_packages(
        &self,
        py: Python,
        path: &Utf8Path,
        session_package: &mut DiscoveredPackage,
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
                let mut package = DiscoveredPackage::new(current_path.to_path_buf());

                let Some(module) =
                    self.discover_test_file(py, &conftest_path, DiscoveryMode::ConfigurationOnly)
                else {
                    break;
                };

                package.add_configuration_module(module);

                session_package.add_package(package);
            }

            if current_path == *self.context.project().cwd() {
                break;
            }

            current_path = match current_path.parent() {
                Some(parent) => parent,
                None => break,
            };
        }
    }

    /// Discovers test files and packages within a directory.
    ///
    /// This method recursively walks through a directory structure to find Python
    /// test files and subdirectories. It respects .gitignore files and filters
    /// out common non-test directories like __pycache__.
    fn discover_directory(
        &self,
        py: Python,
        path: &Utf8PathBuf,
        discovery_mode: DiscoveryMode,
    ) -> DiscoveredPackage {
        let walker = self.create_directory_walker(path);

        let mut package = DiscoveredPackage::new(path.clone());

        for entry in walker {
            let Ok(entry) = entry else {
                continue;
            };
            let current_path = Utf8PathBuf::from_path_buf(entry.path().to_path_buf())
                .expect("Path is not valid UTF-8");

            // Skip the package directory itself
            if package.path() == &current_path {
                continue;
            }

            match entry.file_type() {
                Some(file_type) if file_type.is_dir() => {
                    if discovery_mode.is_configuration_only() {
                        continue;
                    }

                    let subpackage = self.discover_directory(py, &current_path, discovery_mode);

                    package.add_package(subpackage);
                }
                Some(file_type) if file_type.is_file() => match (&current_path).into() {
                    ModuleType::Test => {
                        if discovery_mode.is_configuration_only() {
                            continue;
                        }

                        let Some(module) =
                            self.discover_test_file(py, &current_path, DiscoveryMode::All)
                        else {
                            continue;
                        };

                        package.add_module(module);
                    }
                    ModuleType::Configuration => {
                        let Some(module) = self.discover_test_file(
                            py,
                            &current_path,
                            DiscoveryMode::ConfigurationOnly,
                        ) else {
                            continue;
                        };

                        package.add_configuration_module(module);
                    }
                },
                _ => {}
            }
        }

        package
    }

    /// Creates a configured directory walker for Python file discovery.
    fn create_directory_walker(&self, path: &Utf8PathBuf) -> ignore::Walk {
        WalkBuilder::new(path)
            .max_depth(Some(1))
            .standard_filters(true)
            .require_git(false)
            .git_global(false)
            .parents(true)
            .git_ignore(!self.context.project().options().no_ignore())
            .types({
                let mut types = ignore::types::TypesBuilder::new();
                types.add("python", "*.py").unwrap();
                types.select("python");
                types.build().unwrap()
            })
            .filter_entry(|entry| {
                let file_name = entry.file_name();
                file_name != "__pycache__"
            })
            .build()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DiscoveryMode {
    All,
    ConfigurationOnly,
}

impl DiscoveryMode {
    pub const fn is_configuration_only(self) -> bool {
        matches!(self, Self::ConfigurationOnly)
    }
}

#[cfg(test)]
mod tests {

    use insta::{allow_duplicates, assert_snapshot};
    use karva_project::{Project, ProjectOptions};
    use karva_test::TestContext;

    use super::*;
    use crate::DummyReporter;

    fn session(project: &Project) -> DiscoveredPackage {
        let binding = DummyReporter;
        let context = Context::new(project, &binding);
        let discoverer = StandardDiscoverer::new(&context);
        discoverer.discover()
    }

    #[test]
    fn test_discover_files() {
        let env = TestContext::with_files([("<test>/test.py", "def test_function(): pass")]);

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let session = session(&project);

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <test>.test
                └── test_cases [test_function]
        ");
        assert_eq!(session.total_test_functions(), 1);
    }

    #[test]
    fn test_discover_files_with_directory() {
        let env = TestContext::with_files([
            (
                "<test>/test_dir/test_file1.py",
                "def test_function1(): pass",
            ),
            ("<test>/test_dir/test_file2.py", "def function2(): pass"),
        ]);

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let session = session(&project);

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <temp_dir>/<test>/test_dir/
                └── <test>.test_dir.test_file1
                    └── test_cases [test_function1]
        ");
        assert_eq!(session.total_test_functions(), 1);
    }

    #[test]
    fn test_discover_files_with_gitignore() {
        let env = TestContext::with_files([
            ("<test>/test_file1.py", "def test_function1(): pass"),
            ("<test>/test_file2.py", "def test_function2(): pass"),
            ("<test>/.gitignore", "test_file2.py"),
        ]);

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let session = session(&project);

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <test>.test_file1
                └── test_cases [test_function1]
        ");
        assert_eq!(session.total_test_functions(), 1);
    }

    #[test]
    fn test_discover_files_with_nested_directories() {
        let env = TestContext::with_files([
            ("<test>/test_file1.py", "def test_function1(): pass"),
            ("<test>/nested/test_file2.py", "def test_function2(): pass"),
            (
                "<test>/nested/deeper/test_file3.py",
                "def test_function3(): pass",
            ),
        ]);

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let session = session(&project);

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            ├── <test>.test_file1
            │   └── test_cases [test_function1]
            └── <temp_dir>/<test>/nested/
                ├── <test>.nested.test_file2
                │   └── test_cases [test_function2]
                └── <temp_dir>/<test>/nested/deeper/
                    └── <test>.nested.deeper.test_file3
                        └── test_cases [test_function3]
        ");
        assert_eq!(session.total_test_functions(), 3);
    }

    #[test]
    fn test_discover_files_with_multiple_test_functions() {
        let env = TestContext::with_files([(
            "<test>/test_file.py",
            r"
def test_function1(): pass
def test_function2(): pass
def test_function3(): pass
def not_a_test(): pass
",
        )]);

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let session = session(&project);

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <test>.test_file
                └── test_cases [test_function1 test_function2 test_function3]
        ");
        assert_eq!(session.total_test_functions(), 3);
    }

    #[test]
    fn test_discover_files_with_non_existent_function() {
        let env = TestContext::with_files([("<test>/test_file.py", "def test_function1(): pass")]);

        let project = Project::new(env.cwd(), vec![Utf8PathBuf::from("non_existent_path")]);
        let session = session(&project);

        assert_snapshot!(session.display(), @"");
        assert_eq!(session.total_test_functions(), 0);
    }

    #[test]
    fn test_discover_files_with_invalid_python() {
        let env = TestContext::with_files([("<test>/test_file.py", "test_function1 = None")]);

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let session = session(&project);

        assert_snapshot!(session.display(), @"");
        assert_eq!(session.total_test_functions(), 0);
    }

    #[test]
    fn test_discover_files_with_custom_test_prefix() {
        let env = TestContext::with_files([(
            "<test>/test_file.py",
            r"
def check_function1(): pass
def check_function2(): pass
def test_function(): pass
",
        )]);

        let project = Project::new(env.cwd(), vec![env.cwd()])
            .with_options(ProjectOptions::default().with_test_prefix("check"));

        let session = session(&project);

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <test>.test_file
                └── test_cases [check_function1 check_function2]
        ");
        assert_eq!(session.total_test_functions(), 2);
    }

    #[test]
    fn test_discover_files_with_multiple_paths() {
        let env = TestContext::with_files([
            ("<test>/test1.py", "def test_function1(): pass"),
            ("<test>/test2.py", "def test_function2(): pass"),
            ("<test>/tests/test3.py", "def test_function3(): pass"),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let path_1 = mapped_dir.join("test1.py");
        let path_2 = mapped_dir.join("test2.py");
        let path_3 = mapped_dir.join("tests/test3.py");

        let project = Project::new(env.cwd(), vec![path_1, path_2, path_3]);
        let session = session(&project);

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            ├── <test>.test1
            │   └── test_cases [test_function1]
            ├── <test>.test2
            │   └── test_cases [test_function2]
            └── <temp_dir>/<test>/tests/
                └── <test>.tests.test3
                    └── test_cases [test_function3]
        ");
        assert_eq!(session.total_test_functions(), 3);
    }

    #[test]
    fn test_paths_shadowed_by_other_paths_are_not_discovered_twice() {
        let env = TestContext::with_files([(
            "<test>/test_file.py",
            "def test_function(): pass\ndef test_function2(): pass",
        )]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let path_1 = mapped_dir.join("test_file.py");

        let project = Project::new(env.cwd(), vec![mapped_dir.clone(), path_1]);
        let session = session(&project);
        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <test>.test_file
                └── test_cases [test_function test_function2]
        ");
        assert_eq!(session.total_test_functions(), 2);
    }

    #[test]
    fn test_tests_same_name_different_module_are_discovered() {
        let env = TestContext::with_files([
            ("<test>/test_file.py", "def test_function(): pass"),
            ("<test>/test_file2.py", "def test_function(): pass"),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let path_1 = mapped_dir.join("test_file.py");
        let path_2 = mapped_dir.join("test_file2.py");

        let project = Project::new(env.cwd(), vec![path_1, path_2]);
        let session = session(&project);
        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            ├── <test>.test_file
            │   └── test_cases [test_function]
            └── <test>.test_file2
                └── test_cases [test_function]
        ");
        assert_eq!(session.total_test_functions(), 2);
    }

    #[test]
    fn test_discover_files_with_conftest_explicit_path() {
        let env = TestContext::with_files([
            ("<test>/conftest.py", "def test_function(): pass"),
            ("<test>/test_file.py", "def test_function2(): pass"),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let conftest_path = mapped_dir.join("conftest.py");

        let project = Project::new(env.cwd(), vec![conftest_path]);
        let session = session(&project);

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <test>.conftest
                └── test_cases [test_function]
        ");
        assert_eq!(session.total_test_functions(), 1);
    }

    #[test]
    fn test_discover_files_with_conftest_parent_path_conftest_not_discovered() {
        let env = TestContext::with_files([
            ("<test>/conftest.py", "def test_function(): pass"),
            ("<test>/test_file.py", "def test_function2(): pass"),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let conftest_path = mapped_dir.join("conftest.py");

        let project = Project::new(env.cwd(), vec![conftest_path]);
        let session = session(&project);

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <test>.conftest
                └── test_cases [test_function]
        ");
        assert_eq!(session.total_test_functions(), 1);
    }

    #[test]
    fn test_discover_files_with_cwd_path() {
        let env = TestContext::with_files([("<test>/test_file.py", "def test_function(): pass")]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let path = mapped_dir.join("test_file.py");

        let project = Project::new(env.cwd(), vec![path]);
        let session = session(&project);

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <test>.test_file
                └── test_cases [test_function]
        ");
        assert_eq!(session.total_test_functions(), 1);
    }

    #[test]
    fn test_discover_function_inside_function() {
        let env = TestContext::with_files([(
            "<test>/test_file.py",
            "def test_function(): def test_function2(): pass",
        )]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let path = mapped_dir.join("test_file.py");

        let project = Project::new(env.cwd(), vec![path]);
        let session = session(&project);

        assert_snapshot!(session.display(), @"");
    }

    #[test]
    fn test_discover_fixture_in_same_file_in_root() {
        let env = TestContext::with_files([(
            "<test>/test_1.py",
            r"
import karva
@karva.fixture(scope='function')
def x():
    return 1

def test_1(x): pass",
        )]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let test_path = mapped_dir.join("test_1.py");

        for path in [env.cwd(), test_path] {
            let project = Project::new(env.cwd().clone(), vec![path.clone()]);
            let session = session(&project);

            allow_duplicates! {
                assert_snapshot!(session.display(), @r"
                └── <temp_dir>/<test>/
                    └── <test>.test_1
                        ├── test_cases [test_1]
                        └── fixtures [x]
                ");
            }
        }
    }

    #[test]
    fn test_discover_fixture_in_same_file_in_test_dir() {
        let env = TestContext::with_files([(
            "<test>/tests/test_1.py",
            r"
import karva
@karva.fixture(scope='function')
def x(): return 1
def test_1(x): pass",
        )]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let test_dir = mapped_dir.join("tests");
        let test_path = test_dir.join("test_1.py");

        for path in [env.cwd(), test_dir, test_path] {
            let project = Project::new(env.cwd().clone(), vec![path.clone()]);
            let session = session(&project);
            allow_duplicates! {
                assert_snapshot!(session.display(), @r"
                └── <temp_dir>/<test>/
                    └── <temp_dir>/<test>/tests/
                        └── <test>.tests.test_1
                            ├── test_cases [test_1]
                            └── fixtures [x]
            ")
            };
        }
    }

    #[test]
    fn test_discover_fixture_in_root_tests_in_test_dir() {
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
            ("<test>/tests/test_1.py", "def test_1(x): pass"),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let test_dir = mapped_dir.join("tests");
        let test_path = test_dir.join("test_1.py");

        for path in [env.cwd(), test_dir, test_path] {
            let project = Project::new(env.cwd().clone(), vec![path.clone()]);
            let session = session(&project);

            allow_duplicates! {
                assert_snapshot!(session.display(), @r"
                └── <temp_dir>/<test>/
                    ├── <test>.conftest
                    │   └── fixtures [x]
                    └── <temp_dir>/<test>/tests/
                        └── <test>.tests.test_1
                            └── test_cases [test_1]
                "
                );
            }
        }
    }

    #[test]
    fn test_discover_fixture_in_root_tests_in_nested_dir() {
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
            (
                "<test>/nested_dir/conftest.py",
                r"
import karva
@karva.fixture(scope='function')
def y(x):
    return 2
",
            ),
            (
                "<test>/nested_dir/more_nested_dir/conftest.py",
                r"
import karva
@karva.fixture(scope='function')
def z(x, y):
    return 3
",
            ),
            (
                "<test>/nested_dir/more_nested_dir/even_more_nested_dir/conftest.py",
                r"
import karva
@karva.fixture(scope='function')
def w(x, y, z):
    return 4
",
            ),
            (
                "<test>/nested_dir/more_nested_dir/even_more_nested_dir/test_1.py",
                "def test_1(x): pass",
            ),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let nested_dir = mapped_dir.join("nested_dir");
        let more_nested_dir = nested_dir.join("more_nested_dir");
        let even_more_nested_dir = more_nested_dir.join("even_more_nested_dir");
        let test_path = even_more_nested_dir.join("test_1.py");

        for path in [
            env.cwd(),
            nested_dir,
            more_nested_dir,
            even_more_nested_dir,
            test_path,
        ] {
            let project = Project::new(env.cwd().clone(), vec![path.clone()]);
            let session = session(&project);
            allow_duplicates! {
                assert_snapshot!(session.display(), @r"
                └── <temp_dir>/<test>/
                    ├── <test>.conftest
                    │   └── fixtures [x]
                    └── <temp_dir>/<test>/nested_dir/
                        ├── <test>.nested_dir.conftest
                        │   └── fixtures [y]
                        └── <temp_dir>/<test>/nested_dir/more_nested_dir/
                            ├── <test>.nested_dir.more_nested_dir.conftest
                            │   └── fixtures [z]
                            └── <temp_dir>/<test>/nested_dir/more_nested_dir/even_more_nested_dir/
                                ├── <test>.nested_dir.more_nested_dir.even_more_nested_dir.conftest
                                │   └── fixtures [w]
                                └── <test>.nested_dir.more_nested_dir.even_more_nested_dir.test_1
                                    └── test_cases [test_1]
                ")
            };
        }
    }

    #[test]
    fn test_discover_multiple_test_paths() {
        let env = TestContext::with_files([
            ("<test>/tests/test_1.py", "def test_1(): pass"),
            ("<test>/tests2/test_2.py", "def test_2(): pass"),
            ("<test>/test_3.py", "def test_3(): pass"),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let test_dir_1 = mapped_dir.join("tests");
        let test_dir_2 = mapped_dir.join("tests2");
        let test_file_3 = mapped_dir.join("test_3.py");

        let project = Project::new(env.cwd(), vec![test_dir_1, test_dir_2, test_file_3]);

        let session = session(&project);

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            ├── <test>.test_3
            │   └── test_cases [test_3]
            ├── <temp_dir>/<test>/tests/
            │   └── <test>.tests.test_1
            │       └── test_cases [test_1]
            └── <temp_dir>/<test>/tests2/
                └── <test>.tests2.test_2
                    └── test_cases [test_2]
        ");
    }

    #[test]
    fn test_discover_doubly_nested_with_conftest_middle_path() {
        let env = TestContext::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import karva
@karva.fixture(scope='function')
def root_fixture():
    return 'from_root'
",
            ),
            (
                "<test>/tests/middle_dir/deep_dir/test_nested.py",
                "def test_with_fixture(root_fixture): pass\ndef test_without_fixture(): pass",
            ),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let test_dir = mapped_dir.join("tests");
        let middle_dir = test_dir.join("middle_dir");

        let project = Project::new(env.cwd(), vec![middle_dir]);
        let session = session(&project);

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <temp_dir>/<test>/tests/
                ├── <test>.tests.conftest
                │   └── fixtures [root_fixture]
                └── <temp_dir>/<test>/tests/middle_dir/
                    └── <temp_dir>/<test>/tests/middle_dir/deep_dir/
                        └── <test>.tests.middle_dir.deep_dir.test_nested
                            └── test_cases [test_with_fixture test_without_fixture]
        ");
        assert_eq!(session.total_test_functions(), 2);
    }

    #[test]
    fn test_discover_pytest_fixture() {
        let env = TestContext::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import pytest

@pytest.fixture
def x():
    return 1
",
            ),
            ("<test>/tests/test_1.py", "def test_1(x): pass"),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let test_dir = mapped_dir.join("tests");

        let project = Project::new(env.cwd(), vec![test_dir]);
        let session = session(&project);

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <temp_dir>/<test>/tests/
                ├── <test>.tests.conftest
                │   └── fixtures [x]
                └── <test>.tests.test_1
                    └── test_cases [test_1]
        ");
    }

    #[test]
    fn test_discover_generator_fixture() {
        let env = TestContext::with_files([
            (
                "<test>/conftest.py",
                r"
import karva

@karva.fixture(scope='function')
def x():
    yield 1
",
            ),
            ("<test>/test_1.py", "def test_1(x): pass"),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let conftest_path = mapped_dir.join("conftest.py");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let session = session(&project);

        let mapped_package = session.get_package(mapped_dir).unwrap();

        assert_snapshot!(mapped_package.display(), @r"
        ├── <test>.conftest
        │   └── fixtures [x]
        └── <test>.test_1
            └── test_cases [test_1]
        ");

        let test_1_module = session
            .packages()
            .get(mapped_dir)
            .unwrap()
            .modules()
            .get(&conftest_path)
            .unwrap();

        let fixtures = test_1_module.fixtures();

        let fixture = &fixtures[0];

        assert!(fixture.is_generator());
    }

    #[test]
    fn test_discovery_same_module_given_twice() {
        let env = TestContext::with_files([("<test>/tests/test_1.py", "def test_1(x): pass")]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let test_dir = mapped_dir.join("tests");
        let path = test_dir.join("test_1.py");

        let project = Project::new(env.cwd(), vec![path.clone(), path]);

        let session = session(&project);

        assert_eq!(session.total_test_functions(), 1);
    }

    #[test]
    fn test_nested_function_not_discovered() {
        let env = TestContext::with_files([(
            "<test>/test_file.py",
            "
            def test_1():
                def test_2(): pass

                ",
        )]);

        let project = Project::new(env.cwd(), vec![env.cwd()]);

        let session = session(&project);

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <test>.test_file
                └── test_cases [test_1]
        ");
    }
}
