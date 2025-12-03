use pyo3::prelude::*;

#[cfg(test)]
use crate::utils::attach_with_project;
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

    #[cfg(test)]
    pub(crate) fn discover(self) -> DiscoveredPackage {
        attach_with_project(self.context.project(), |py| self.discover_with_py(py))
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

#[cfg(test)]
mod tests {

    use camino::Utf8PathBuf;
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
            "
def test_function():
    def test_function2(): pass",
        )]);

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let session = session(&project);

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <test>.test_file
                └── test_cases [test_function]
        ");
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
