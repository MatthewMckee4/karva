use ignore::WalkBuilder;
use karva_project::{
    Project,
    path::{SystemPathBuf, TestPath},
    utils::is_python_file,
};
use pyo3::prelude::*;

use crate::{
    diagnostic::Diagnostic,
    discovery::{DiscoveredModule, DiscoveredPackage, ModuleType, discover},
    utils::add_to_sys_path,
};

pub(crate) struct StandardDiscoverer<'proj> {
    project: &'proj Project,
}

impl<'proj> StandardDiscoverer<'proj> {
    #[must_use]
    pub(crate) const fn new(project: &'proj Project) -> Self {
        Self { project }
    }

    #[must_use]
    pub(crate) fn discover(self, py: Python<'_>) -> (DiscoveredPackage, Vec<Diagnostic>) {
        let mut session_package = DiscoveredPackage::new(self.project.cwd().clone());

        let mut discovery_diagnostics = Vec::new();
        let cwd = self.project.cwd();

        if add_to_sys_path(py, cwd, 0).is_err() {
            return (session_package, discovery_diagnostics);
        }

        tracing::info!("Discovering tests...");

        for path in self.project.test_paths() {
            match path {
                Ok(path) => {
                    match &path {
                        TestPath::File(path) => {
                            let module = self.discover_test_file(
                                py,
                                path,
                                &mut discovery_diagnostics,
                                false,
                            );

                            if let Some(module) = module {
                                session_package.add_module(module);
                            }
                        }
                        TestPath::Directory(path) => {
                            let mut package = DiscoveredPackage::new(path.clone());

                            self.discover_directory(
                                py,
                                &mut package,
                                &mut discovery_diagnostics,
                                false,
                            );
                            session_package.add_package(package);
                        }
                        TestPath::Function {
                            path,
                            function_name,
                        } => {
                            let module = self.discover_test_file(
                                py,
                                path,
                                &mut discovery_diagnostics,
                                false,
                            );

                            if let Some(mut module) = module {
                                let test_functions = module.test_functions();

                                let filtered_functions: Vec<_> = test_functions
                                    .into_iter()
                                    .filter(|func| func.name() == *function_name)
                                    .cloned()
                                    .collect();

                                if !filtered_functions.is_empty() {
                                    module.set_test_functions(filtered_functions);
                                    session_package.add_module(module);
                                }
                            }
                        }
                    }

                    self.add_parent_configuration_packages(
                        py,
                        path.path(),
                        &mut session_package,
                        &mut discovery_diagnostics,
                    );
                }
                Err(e) => {
                    discovery_diagnostics.push(Diagnostic::invalid_path_error(&e));
                }
            }
        }

        session_package.shrink();

        (session_package, discovery_diagnostics)
    }

    // Parse and run discovery on a single file
    fn discover_test_file(
        &self,
        py: Python<'_>,
        path: &SystemPathBuf,
        discovery_diagnostics: &mut Vec<Diagnostic>,
        configuration_only: bool,
    ) -> Option<DiscoveredModule> {
        tracing::debug!("Discovering file: {}", path.display());

        if !is_python_file(path) {
            return None;
        }

        let module_type = ModuleType::from_path(path);

        let mut module = DiscoveredModule::new(self.project, path, module_type);

        let (discovered, diagnostics) = discover(py, &module, self.project);

        if !configuration_only {
            module.set_test_functions(discovered.functions);
        }
        module.set_fixtures(discovered.fixtures);

        discovery_diagnostics.extend(diagnostics);

        if module.is_empty() {
            return None;
        }

        Some(module)
    }

    // This should look from the parent of path to the cwd for configuration files
    fn add_parent_configuration_packages(
        &self,
        py: Python<'_>,
        path: &SystemPathBuf,
        session_package: &mut DiscoveredPackage,
        discovery_diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<()> {
        let mut current_path = if path.is_dir() {
            path.clone()
        } else {
            path.parent()?.to_path_buf()
        };

        loop {
            let mut package = DiscoveredPackage::new(current_path.clone());

            self.discover_directory(py, &mut package, discovery_diagnostics, true);

            session_package.add_package(package);

            if current_path == *self.project.cwd() {
                break;
            }
            current_path = current_path.parent()?.to_path_buf();
        }

        Some(())
    }

    // Parse and run discovery on a directory
    //
    // If configuration_only is true, only discover configuration files
    fn discover_directory(
        &self,
        py: Python<'_>,
        package: &mut DiscoveredPackage,
        discovery_diagnostics: &mut Vec<Diagnostic>,
        configuration_only: bool,
    ) {
        tracing::debug!("Discovering directory: {}", package.path().display());

        let walker = WalkBuilder::new(package.path())
            .max_depth(Some(1))
            .standard_filters(true)
            .require_git(false)
            .git_global(false)
            .parents(true)
            .build();

        for entry in walker {
            let Ok(entry) = entry else { continue };

            let current_path = SystemPathBuf::from(entry.path());

            if package.path() == &current_path {
                continue;
            }

            match entry.file_type() {
                Some(file_type) if file_type.is_dir() => {
                    if configuration_only {
                        continue;
                    }

                    let mut subpackage = DiscoveredPackage::new(current_path.clone());

                    self.discover_directory(
                        py,
                        &mut subpackage,
                        discovery_diagnostics,
                        configuration_only,
                    );
                    package.add_package(subpackage);
                }
                Some(file_type) if file_type.is_file() => {
                    match ModuleType::from_path(&current_path) {
                        ModuleType::Test => {
                            if configuration_only {
                                continue;
                            }
                            if let Some(module) = self.discover_test_file(
                                py,
                                &current_path,
                                discovery_diagnostics,
                                false,
                            ) {
                                package.add_module(module);
                            }
                        }
                        ModuleType::Configuration => {
                            if let Some(module) = self.discover_test_file(
                                py,
                                &current_path,
                                discovery_diagnostics,
                                true,
                            ) {
                                package.add_configuration_module(module);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use insta::{allow_duplicates, assert_snapshot};
    use karva_project::{project::ProjectOptions, testing::TestEnv, verbosity::VerbosityLevel};

    use super::*;

    #[test]
    fn test_discover_files() {
        let env = TestEnv::with_files([("<test>/test.py", "def test_function(): pass")]);

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let discoverer = StandardDiscoverer::new(&project);
        let (session, _) = Python::with_gil(|py| discoverer.discover(py));

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <test>.test
                ├── test_cases [test_function]
                └── fixtures []
        ");
        assert_eq!(session.total_test_functions(), 1);
    }

    #[test]
    fn test_discover_files_with_directory() {
        let env = TestEnv::with_files([
            (
                "<test>/test_dir/test_file1.py",
                "def test_function1(): pass",
            ),
            ("<test>/test_dir/test_file2.py", "def function2(): pass"),
        ]);

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let discoverer = StandardDiscoverer::new(&project);
        let (session, _) = Python::with_gil(|py| discoverer.discover(py));

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <temp_dir>/<test>/test_dir/
                └── <test>.test_dir.test_file1
                    ├── test_cases [test_function1]
                    └── fixtures []
        ");
        assert_eq!(session.total_test_functions(), 1);
    }

    #[test]
    fn test_discover_files_with_gitignore() {
        let env = TestEnv::with_files([
            ("<test>/test_file1.py", "def test_function1(): pass"),
            ("<test>/test_file2.py", "def test_function2(): pass"),
            ("<test>/.gitignore", "test_file2.py"),
        ]);

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let discoverer = StandardDiscoverer::new(&project);
        let (session, _) = Python::with_gil(|py| discoverer.discover(py));

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <test>.test_file1
                ├── test_cases [test_function1]
                └── fixtures []
        ");
        assert_eq!(session.total_test_functions(), 1);
    }

    #[test]
    fn test_discover_files_with_nested_directories() {
        let env = TestEnv::with_files([
            ("<test>/test_file1.py", "def test_function1(): pass"),
            ("<test>/nested/test_file2.py", "def test_function2(): pass"),
            (
                "<test>/nested/deeper/test_file3.py",
                "def test_function3(): pass",
            ),
        ]);

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let discoverer = StandardDiscoverer::new(&project);
        let (session, _) = Python::with_gil(|py| discoverer.discover(py));

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            ├── <test>.test_file1
            │   ├── test_cases [test_function1]
            │   └── fixtures []
            └── <temp_dir>/<test>/nested/
                ├── <test>.nested.test_file2
                │   ├── test_cases [test_function2]
                │   └── fixtures []
                └── <temp_dir>/<test>/nested/deeper/
                    └── <test>.nested.deeper.test_file3
                        ├── test_cases [test_function3]
                        └── fixtures []
        ");
        assert_eq!(session.total_test_functions(), 3);
    }

    #[test]
    fn test_discover_files_with_multiple_test_functions() {
        let env = TestEnv::with_files([(
            "<test>/test_file.py",
            r"
def test_function1(): pass
def test_function2(): pass
def test_function3(): pass
def not_a_test(): pass
",
        )]);

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let discoverer = StandardDiscoverer::new(&project);
        let (session, _) = Python::with_gil(|py| discoverer.discover(py));

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <test>.test_file
                ├── test_cases [test_function1 test_function2 test_function3]
                └── fixtures []
        ");
        assert_eq!(session.total_test_functions(), 3);
    }

    #[test]
    fn test_discover_files_with_non_existent_function() {
        let env = TestEnv::with_files([("<test>/test_file.py", "def test_function1(): pass")]);

        let project = Project::new(env.cwd(), vec![SystemPathBuf::from("non_existent_path")]);
        let discoverer = StandardDiscoverer::new(&project);
        let (session, _) = Python::with_gil(|py| discoverer.discover(py));

        assert_snapshot!(session.display(), @"");
        assert_eq!(session.total_test_functions(), 0);
    }

    #[test]
    fn test_discover_files_with_invalid_python() {
        let env = TestEnv::with_files([("<test>/test_file.py", "test_function1 = None")]);

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let discoverer = StandardDiscoverer::new(&project);
        let (session, _) = Python::with_gil(|py| discoverer.discover(py));

        assert_snapshot!(session.display(), @"");
        assert_eq!(session.total_test_functions(), 0);
    }

    #[test]
    fn test_discover_files_with_custom_test_prefix() {
        let env = TestEnv::with_files([(
            "<test>/test_file.py",
            r"
def check_function1(): pass
def check_function2(): pass
def test_function(): pass
",
        )]);

        let project = Project::new(env.cwd(), vec![env.cwd()]).with_options(ProjectOptions::new(
            "check".to_string(),
            VerbosityLevel::Default,
            false,
        ));

        let discoverer = StandardDiscoverer::new(&project);
        let (session, _) = Python::with_gil(|py| discoverer.discover(py));

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <test>.test_file
                ├── test_cases [check_function1 check_function2]
                └── fixtures []
        ");
        assert_eq!(session.total_test_functions(), 2);
    }

    #[test]
    fn test_discover_files_with_multiple_paths() {
        let env = TestEnv::with_files([
            ("<test>/test1.py", "def test_function1(): pass"),
            ("<test>/test2.py", "def test_function2(): pass"),
            ("<test>/tests/test3.py", "def test_function3(): pass"),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let path_1 = mapped_dir.join("test1.py");
        let path_2 = mapped_dir.join("test2.py");
        let path_3 = mapped_dir.join("tests/test3.py");

        let project = Project::new(env.cwd(), vec![path_1, path_2, path_3]);
        let discoverer = StandardDiscoverer::new(&project);
        let (session, _) = Python::with_gil(|py| discoverer.discover(py));

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            ├── <test>.test1
            │   ├── test_cases [test_function1]
            │   └── fixtures []
            ├── <test>.test2
            │   ├── test_cases [test_function2]
            │   └── fixtures []
            └── <temp_dir>/<test>/tests/
                └── <test>.tests.test3
                    ├── test_cases [test_function3]
                    └── fixtures []
        ");
        assert_eq!(session.total_test_functions(), 3);
    }

    #[test]
    fn test_paths_shadowed_by_other_paths_are_not_discovered_twice() {
        let env = TestEnv::with_files([(
            "<test>/test_file.py",
            "def test_function(): pass\ndef test_function2(): pass",
        )]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let path_1 = mapped_dir.join("test_file.py");

        let project = Project::new(env.cwd(), vec![mapped_dir.clone(), path_1]);
        let discoverer = StandardDiscoverer::new(&project);
        let (session, _) = Python::with_gil(|py| discoverer.discover(py));
        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <test>.test_file
                ├── test_cases [test_function test_function2]
                └── fixtures []
        ");
        assert_eq!(session.total_test_functions(), 2);
    }

    #[test]
    fn test_tests_same_name_different_module_are_discovered() {
        let env = TestEnv::with_files([
            ("<test>/test_file.py", "def test_function(): pass"),
            ("<test>/test_file2.py", "def test_function(): pass"),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let path_1 = mapped_dir.join("test_file.py");
        let path_2 = mapped_dir.join("test_file2.py");

        let project = Project::new(env.cwd(), vec![path_1, path_2]);
        let discoverer = StandardDiscoverer::new(&project);
        let (session, _) = Python::with_gil(|py| discoverer.discover(py));
        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            ├── <test>.test_file
            │   ├── test_cases [test_function]
            │   └── fixtures []
            └── <test>.test_file2
                ├── test_cases [test_function]
                └── fixtures []
        ");
        assert_eq!(session.total_test_functions(), 2);
    }

    #[test]
    fn test_discover_files_with_conftest_explicit_path() {
        let env = TestEnv::with_files([
            ("<test>/conftest.py", "def test_function(): pass"),
            ("<test>/test_file.py", "def test_function2(): pass"),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let conftest_path = mapped_dir.join("conftest.py");

        let project = Project::new(env.cwd(), vec![conftest_path]);
        let discoverer = StandardDiscoverer::new(&project);
        let (session, _) = Python::with_gil(|py| discoverer.discover(py));

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <test>.conftest
                ├── test_cases [test_function]
                └── fixtures []
        ");
        assert_eq!(session.total_test_functions(), 1);
    }

    #[test]
    fn test_discover_files_with_conftest_parent_path_conftest_not_discovered() {
        let env = TestEnv::with_files([
            ("<test>/conftest.py", "def test_function(): pass"),
            ("<test>/test_file.py", "def test_function2(): pass"),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let conftest_path = mapped_dir.join("conftest.py");

        let project = Project::new(env.cwd(), vec![conftest_path]);
        let discoverer = StandardDiscoverer::new(&project);
        let (session, _) = Python::with_gil(|py| discoverer.discover(py));

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <test>.conftest
                ├── test_cases [test_function]
                └── fixtures []
        ");
        assert_eq!(session.total_test_functions(), 1);
    }

    #[test]
    fn test_discover_files_with_cwd_path() {
        let env = TestEnv::with_files([("<test>/test_file.py", "def test_function(): pass")]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let path = mapped_dir.join("test_file.py");

        let project = Project::new(env.cwd(), vec![path]);
        let discoverer = StandardDiscoverer::new(&project);
        let (session, _) = Python::with_gil(|py| discoverer.discover(py));

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <test>.test_file
                ├── test_cases [test_function]
                └── fixtures []
        ");
        assert_eq!(session.total_test_functions(), 1);
    }

    #[test]
    fn test_discover_function_inside_function() {
        let env = TestEnv::with_files([(
            "<test>/test_file.py",
            "def test_function(): def test_function2(): pass",
        )]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let path = mapped_dir.join("test_file.py");

        let project = Project::new(env.cwd(), vec![path]);
        let discoverer = StandardDiscoverer::new(&project);

        let (session, _) = Python::with_gil(|py| discoverer.discover(py));

        assert_snapshot!(session.display(), @"");
    }

    #[test]
    fn test_discover_fixture_in_same_file_in_root() {
        let env = TestEnv::with_files([(
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
            let (session, _) =
                Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

            allow_duplicates!(assert_snapshot!(session.display(), @r"
            └── <temp_dir>/<test>/
                └── <test>.test_1
                    ├── test_cases [test_1]
                    └── fixtures [x]
            "));
        }
    }

    #[test]
    fn test_discover_fixture_in_same_file_in_test_dir() {
        let env = TestEnv::with_files([(
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
            let (session, _) =
                Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));
            allow_duplicates!(assert_snapshot!(session.display(), @r"
            └── <temp_dir>/<test>/
                └── <temp_dir>/<test>/tests/
                    └── <test>.tests.test_1
                        ├── test_cases [test_1]
                        └── fixtures [x]
            "));
        }
    }

    #[test]
    fn test_discover_fixture_in_root_tests_in_test_dir() {
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
            ("<test>/tests/test_1.py", "def test_1(x): pass"),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let test_dir = mapped_dir.join("tests");
        let test_path = test_dir.join("test_1.py");

        for path in [env.cwd(), test_dir, test_path] {
            let project = Project::new(env.cwd().clone(), vec![path.clone()]);
            let (session, _) =
                Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

            allow_duplicates!(assert_snapshot!(session.display(), @r"
            └── <temp_dir>/<test>/
                ├── <test>.conftest
                │   ├── test_cases []
                │   └── fixtures [x]
                └── <temp_dir>/<test>/tests/
                    └── <test>.tests.test_1
                        ├── test_cases [test_1]
                        └── fixtures []
            "));
        }
    }

    #[test]
    fn test_discover_fixture_in_root_tests_in_nested_dir() {
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
            let (session, _) =
                Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));
            allow_duplicates!(assert_snapshot!(session.display(), @r"
            └── <temp_dir>/<test>/
                ├── <test>.conftest
                │   ├── test_cases []
                │   └── fixtures [x]
                └── <temp_dir>/<test>/nested_dir/
                    ├── <test>.nested_dir.conftest
                    │   ├── test_cases []
                    │   └── fixtures [y]
                    └── <temp_dir>/<test>/nested_dir/more_nested_dir/
                        ├── <test>.nested_dir.more_nested_dir.conftest
                        │   ├── test_cases []
                        │   └── fixtures [z]
                        └── <temp_dir>/<test>/nested_dir/more_nested_dir/even_more_nested_dir/
                            ├── <test>.nested_dir.more_nested_dir.even_more_nested_dir.conftest
                            │   ├── test_cases []
                            │   └── fixtures [w]
                            └── <test>.nested_dir.more_nested_dir.even_more_nested_dir.test_1
                                ├── test_cases [test_1]
                                └── fixtures []
            "));
        }
    }

    #[test]
    fn test_discover_multiple_test_paths() {
        let env = TestEnv::with_files([
            ("<test>/tests/test_1.py", "def test_1(): pass"),
            ("<test>/tests2/test_2.py", "def test_2(): pass"),
            ("<test>/test_3.py", "def test_3(): pass"),
        ]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let test_dir_1 = mapped_dir.join("tests");
        let test_dir_2 = mapped_dir.join("tests2");
        let test_file_3 = mapped_dir.join("test_3.py");

        let project = Project::new(env.cwd(), vec![test_dir_1, test_dir_2, test_file_3]);

        let (session, _) = Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            ├── <test>.test_3
            │   ├── test_cases [test_3]
            │   └── fixtures []
            ├── <temp_dir>/<test>/tests/
            │   └── <test>.tests.test_1
            │       ├── test_cases [test_1]
            │       └── fixtures []
            └── <temp_dir>/<test>/tests2/
                └── <test>.tests2.test_2
                    ├── test_cases [test_2]
                    └── fixtures []
        ");
    }

    #[test]
    fn test_discover_doubly_nested_with_conftest_middle_path() {
        let env = TestEnv::with_files([
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
        let (session, _) = Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <temp_dir>/<test>/tests/
                ├── <test>.tests.conftest
                │   ├── test_cases []
                │   └── fixtures [root_fixture]
                └── <temp_dir>/<test>/tests/middle_dir/
                    └── <temp_dir>/<test>/tests/middle_dir/deep_dir/
                        └── <test>.tests.middle_dir.deep_dir.test_nested
                            ├── test_cases [test_with_fixture test_without_fixture]
                            └── fixtures []
        ");
        assert_eq!(session.total_test_functions(), 2);
    }

    #[test]
    fn test_discover_pytest_fixture() {
        let env = TestEnv::with_files([
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
        let (session, _) = Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

        assert_snapshot!(session.display(), @r"
        └── <temp_dir>/<test>/
            └── <temp_dir>/<test>/tests/
                ├── <test>.tests.conftest
                │   ├── test_cases []
                │   └── fixtures [x]
                └── <test>.tests.test_1
                    ├── test_cases [test_1]
                    └── fixtures []
        ");
    }

    #[test]
    fn test_discover_generator_fixture() {
        let env = TestEnv::with_files([
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
        let (session, _) = Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

        let mapped_package = session.get_package(mapped_dir).unwrap();

        assert_snapshot!(mapped_package.display(), @r"
        ├── <test>.conftest
        │   ├── test_cases []
        │   └── fixtures [x]
        └── <test>.test_1
            ├── test_cases [test_1]
            └── fixtures []
        ");

        let test_1_module = session
            .packages()
            .get(mapped_dir)
            .unwrap()
            .modules()
            .get(&conftest_path)
            .unwrap();

        let fixture = test_1_module.fixtures()[0];

        assert!(fixture.is_generator());
    }

    #[test]
    fn test_discovery_same_module_given_twice() {
        let env = TestEnv::with_files([("<test>/tests/test_1.py", "def test_1(x): pass")]);

        let mapped_dir = env.mapped_path("<test>").unwrap();
        let test_dir = mapped_dir.join("tests");
        let path = test_dir.join("test_1.py");

        let project = Project::new(env.cwd(), vec![path.clone(), path]);

        let (session, _) = Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

        assert_eq!(session.total_test_functions(), 1);
    }
}
