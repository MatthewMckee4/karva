use ignore::WalkBuilder;
use karva_project::{
    path::{SystemPathBuf, TestPath},
    project::Project,
    utils::is_python_file,
};
use pyo3::prelude::*;

use crate::{
    diagnostic::{Diagnostic, DiagnosticScope, ErrorType, Severity},
    discovery::discover,
    models::{Module, ModuleType, Package},
    utils::{add_to_sys_path, with_gil},
};

pub struct Discoverer<'proj> {
    project: &'proj Project,
}

impl<'proj> Discoverer<'proj> {
    #[must_use]
    pub const fn new(project: &'proj Project) -> Self {
        Self { project }
    }

    #[must_use]
    pub fn discover(self) -> (Package<'proj>, Vec<Diagnostic>) {
        let mut session_package = Package::new(self.project.cwd().clone(), self.project);

        let mut discovery_diagnostics = Vec::new();
        with_gil(self.project, |py| {
            let cwd = self.project.cwd();

            if let Err(err) = add_to_sys_path(&py, cwd) {
                discovery_diagnostics.push(Diagnostic::from_py_err(
                    py,
                    &err,
                    DiagnosticScope::Setup,
                    Some(cwd.to_string()),
                    Severity::Error(ErrorType::Known("invalid-path".to_string())),
                ));
                return;
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
                                    &session_package,
                                    &mut discovery_diagnostics,
                                    false,
                                );
                                if let Some(module) = module {
                                    session_package.add_module(module);
                                }
                            }
                            TestPath::Directory(path) => {
                                let mut package = Package::new(path.clone(), self.project);

                                self.discover_directory(
                                    py,
                                    &mut package,
                                    &session_package,
                                    &mut discovery_diagnostics,
                                    false,
                                );
                                session_package.add_package(package);
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
        });

        session_package.shrink();

        (session_package, discovery_diagnostics)
    }

    // Parse and run discovery on a single file
    fn discover_test_file(
        &self,
        py: Python<'_>,
        path: &SystemPathBuf,
        session_package: &Package<'proj>,
        discovery_diagnostics: &mut Vec<Diagnostic>,
        configuration_only: bool,
    ) -> Option<Module<'proj>> {
        tracing::debug!("Discovering file: {}", path);

        if !is_python_file(path) {
            return None;
        }

        if session_package.contains_path(path) {
            return None;
        }

        let module_type = ModuleType::from_path(path);

        let mut module = Module::new(self.project, path, module_type);

        let (discovered, diagnostics) = discover(py, path, self.project);

        if !configuration_only {
            module.set_test_cases(discovered.functions);
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
        session_package: &mut Package<'proj>,
        discovery_diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<()> {
        let mut current_path = path.clone();

        loop {
            let mut package = Package::new(current_path.clone(), self.project);
            self.discover_directory(
                py,
                &mut package,
                session_package,
                discovery_diagnostics,
                true,
            );
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
        package: &mut Package<'proj>,
        session_package: &Package<'proj>,
        discovery_diagnostics: &mut Vec<Diagnostic>,
        configuration_only: bool,
    ) {
        tracing::debug!("Discovering directory: {}", package.path());

        let walker = WalkBuilder::new(package.path().as_std_path())
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

            if session_package.contains_path(&current_path) {
                continue;
            }

            match entry.file_type() {
                Some(file_type) if file_type.is_dir() => {
                    if configuration_only {
                        continue;
                    }

                    let mut subpackage = Package::new(current_path.clone(), self.project);
                    self.discover_directory(
                        py,
                        &mut subpackage,
                        session_package,
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
                                session_package,
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
                                session_package,
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

    use std::collections::{HashMap, HashSet};

    use karva_project::{project::ProjectOptions, tests::TestEnv, verbosity::VerbosityLevel};

    use super::*;
    use crate::models::{StringModule, StringPackage};

    #[test]
    fn test_discover_files() {
        let env = TestEnv::new();
        let path = env.create_file("test.py", "def test_function(): pass");

        let project = Project::new(env.cwd(), vec![path]);
        let discoverer = Discoverer::new(&project);
        let (session, _) = discoverer.discover();

        assert_eq!(
            session.display(),
            StringPackage {
                modules: HashMap::from([(
                    "test".to_string(),
                    StringModule {
                        test_cases: HashSet::from(["test_function".to_string()]),
                        fixtures: HashSet::new(),
                    },
                )]),
                packages: HashMap::new(),
            }
        );
        assert_eq!(session.total_test_cases(), 1);
    }

    #[test]
    fn test_discover_files_with_directory() {
        let env = TestEnv::new();
        let path = env.create_dir("test_dir");

        env.create_file(
            path.join("test_file1.py").as_std_path(),
            "def test_function1(): pass",
        );
        env.create_file(
            path.join("test_file2.py").as_std_path(),
            "def function2(): pass",
        );

        let project = Project::new(env.cwd(), vec![path]);
        let discoverer = Discoverer::new(&project);
        let (session, _) = discoverer.discover();

        assert_eq!(
            session.display(),
            StringPackage {
                modules: HashMap::new(),
                packages: HashMap::from([(
                    "test_dir".to_string(),
                    StringPackage {
                        modules: HashMap::from([(
                            "test_file1".to_string(),
                            StringModule {
                                test_cases: HashSet::from(["test_function1".to_string(),]),
                                fixtures: HashSet::new(),
                            },
                        )]),
                        packages: HashMap::new(),
                    }
                )]),
            }
        );
        assert_eq!(session.total_test_cases(), 1);
    }

    #[test]
    fn test_discover_files_with_gitignore() {
        let env = TestEnv::new();
        let test_dir = env.create_tests_dir();

        env.create_file(
            test_dir.join("test_file1.py").as_std_path(),
            "def test_function1(): pass",
        );
        env.create_file(
            test_dir.join("test_file2.py").as_std_path(),
            "def test_function2(): pass",
        );
        env.create_file(test_dir.join(".gitignore").as_std_path(), "test_file2.py");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let discoverer = Discoverer::new(&project);
        let (session, _) = discoverer.discover();

        assert_eq!(
            session.display(),
            StringPackage {
                modules: HashMap::new(),
                packages: HashMap::from([(
                    test_dir.strip_prefix(env.cwd()).unwrap().to_string(),
                    StringPackage {
                        modules: HashMap::from([(
                            "test_file1".to_string(),
                            StringModule {
                                test_cases: HashSet::from(["test_function1".to_string()]),
                                fixtures: HashSet::new(),
                            },
                        )]),
                        packages: HashMap::new(),
                    }
                ),]),
            }
        );
        assert_eq!(session.total_test_cases(), 1);
    }

    #[test]
    fn test_discover_files_with_nested_directories() {
        let env = TestEnv::new();
        let test_dir = env.create_tests_dir();
        env.create_dir(test_dir.join("nested").as_std_path());
        env.create_dir(test_dir.join("nested/deeper").as_std_path());

        env.create_file(
            test_dir.join("test_file1.py").as_std_path(),
            "def test_function1(): pass",
        );
        env.create_file(
            test_dir.join("nested/test_file2.py").as_std_path(),
            "def test_function2(): pass",
        );
        env.create_file(
            test_dir.join("nested/deeper/test_file3.py").as_std_path(),
            "def test_function3(): pass",
        );

        let project = Project::new(env.cwd(), vec![test_dir.clone()]);
        let discoverer = Discoverer::new(&project);
        let (session, _) = discoverer.discover();

        assert_eq!(
            session.display(),
            StringPackage {
                modules: HashMap::new(),
                packages: HashMap::from([(
                    test_dir.strip_prefix(env.cwd()).unwrap().to_string(),
                    StringPackage {
                        modules: HashMap::from([(
                            "test_file1".to_string(),
                            StringModule {
                                test_cases: HashSet::from(["test_function1".to_string(),]),
                                fixtures: HashSet::new(),
                            },
                        )]),
                        packages: HashMap::from([(
                            "nested".to_string(),
                            StringPackage {
                                modules: HashMap::from([(
                                    "test_file2".to_string(),
                                    StringModule {
                                        test_cases: HashSet::from(["test_function2".to_string(),]),
                                        fixtures: HashSet::new(),
                                    },
                                )]),
                                packages: HashMap::from([(
                                    "deeper".to_string(),
                                    StringPackage {
                                        modules: HashMap::from([(
                                            "test_file3".to_string(),
                                            StringModule {
                                                test_cases: HashSet::from([
                                                    "test_function3".to_string(),
                                                ]),
                                                fixtures: HashSet::new(),
                                            },
                                        )]),
                                        packages: HashMap::new(),
                                    }
                                )]),
                            }
                        )]),
                    }
                )]),
            }
        );
        assert_eq!(session.total_test_cases(), 3);
    }

    #[test]
    fn test_discover_files_with_multiple_test_functions() {
        let env = TestEnv::new();
        let path = env.create_file(
            "test_file.py",
            r"
def test_function1(): pass
def test_function2(): pass
def test_function3(): pass
def not_a_test(): pass
",
        );

        let project = Project::new(env.cwd(), vec![path]);
        let discoverer = Discoverer::new(&project);
        let (session, _) = discoverer.discover();

        assert_eq!(
            session.display(),
            StringPackage {
                modules: HashMap::from([(
                    "test_file".to_string(),
                    StringModule {
                        test_cases: HashSet::from([
                            "test_function1".to_string(),
                            "test_function2".to_string(),
                            "test_function3".to_string(),
                        ]),
                        fixtures: HashSet::new(),
                    },
                )]),
                packages: HashMap::new(),
            }
        );
        assert_eq!(session.total_test_cases(), 3);
    }

    #[test]
    fn test_discover_files_with_nonexistent_function() {
        let env = TestEnv::new();
        let path = env.create_file("test_file.py", "def test_function1(): pass");

        let project = Project::new(env.cwd(), vec![path.join("nonexistent_function")]);
        let discoverer = Discoverer::new(&project);
        let (session, _) = discoverer.discover();

        assert_eq!(
            session.display(),
            StringPackage {
                modules: HashMap::new(),
                packages: HashMap::new(),
            }
        );
        assert_eq!(session.total_test_cases(), 0);
    }

    #[test]
    fn test_discover_files_with_invalid_python() {
        let env = TestEnv::new();
        let path = env.create_file("test_file.py", "test_function1 = None");

        let project = Project::new(env.cwd(), vec![path]);
        let discoverer = Discoverer::new(&project);
        let (session, _) = discoverer.discover();

        assert_eq!(
            session.display(),
            StringPackage {
                modules: HashMap::new(),
                packages: HashMap::new(),
            }
        );
        assert_eq!(session.total_test_cases(), 0);
    }

    #[test]
    fn test_discover_files_with_custom_test_prefix() {
        let env = TestEnv::new();
        let path = env.create_file(
            "test_file.py",
            r"
def check_function1(): pass
def check_function2(): pass
def test_function(): pass
",
        );

        let project = Project::new(env.cwd(), vec![path]).with_options(ProjectOptions::new(
            "check".to_string(),
            VerbosityLevel::Default,
            false,
        ));
        let discoverer = Discoverer::new(&project);
        let (session, _) = discoverer.discover();

        assert_eq!(
            session.display(),
            StringPackage {
                modules: HashMap::from([(
                    "test_file".to_string(),
                    StringModule {
                        test_cases: HashSet::from([
                            "check_function1".to_string(),
                            "check_function2".to_string(),
                        ]),
                        fixtures: HashSet::new(),
                    },
                )]),
                packages: HashMap::new(),
            }
        );
        assert_eq!(session.total_test_cases(), 2);
    }

    #[test]
    fn test_discover_files_with_multiple_paths() {
        let env = TestEnv::new();
        let file1 = env.create_file("test1.py", "def test_function1(): pass");
        let file2 = env.create_file("test2.py", "def test_function2(): pass");
        let test_dir = env.create_tests_dir();
        env.create_file(
            test_dir.join("test3.py").as_std_path(),
            "def test_function3(): pass",
        );

        let project = Project::new(env.cwd(), vec![file1, file2, test_dir.clone()]);
        let discoverer = Discoverer::new(&project);
        let (session, _) = discoverer.discover();

        assert_eq!(
            session.display(),
            StringPackage {
                modules: HashMap::from([
                    (
                        "test1".to_string(),
                        StringModule {
                            test_cases: HashSet::from(["test_function1".to_string(),]),
                            fixtures: HashSet::new(),
                        },
                    ),
                    (
                        "test2".to_string(),
                        StringModule {
                            test_cases: HashSet::from(["test_function2".to_string(),]),
                            fixtures: HashSet::new(),
                        },
                    )
                ]),
                packages: HashMap::from([(
                    test_dir.strip_prefix(env.cwd()).unwrap().to_string(),
                    StringPackage {
                        modules: HashMap::from([(
                            "test3".to_string(),
                            StringModule {
                                test_cases: HashSet::from(["test_function3".to_string(),]),
                                fixtures: HashSet::new(),
                            },
                        )]),
                        packages: HashMap::new(),
                    }
                )]),
            }
        );
        assert_eq!(session.total_test_cases(), 3);
    }

    #[test]
    fn test_paths_shadowed_by_other_paths_are_not_discovered_twice() {
        let env = TestEnv::new();
        let test_dir = env.create_tests_dir();
        let path = env.create_file(
            test_dir.join("test_file.py").as_std_path(),
            "def test_function(): pass\ndef test_function2(): pass",
        );

        let project = Project::new(env.cwd(), vec![path, test_dir.clone()]);
        let discoverer = Discoverer::new(&project);
        let (session, _) = discoverer.discover();
        assert_eq!(
            session.display(),
            StringPackage {
                modules: HashMap::new(),
                packages: HashMap::from([(
                    test_dir.strip_prefix(env.cwd()).unwrap().to_string(),
                    StringPackage {
                        modules: HashMap::from([(
                            "test_file".to_string(),
                            StringModule {
                                test_cases: HashSet::from([
                                    "test_function".to_string(),
                                    "test_function2".to_string(),
                                ]),
                                fixtures: HashSet::new(),
                            },
                        )]),
                        packages: HashMap::new(),
                    }
                )]),
            }
        );
        assert_eq!(session.total_test_cases(), 2);
    }

    #[test]
    fn test_tests_same_name_different_module_are_discovered() {
        let env = TestEnv::new();
        let test_dir = env.create_tests_dir();
        let path = env.create_file(
            test_dir.join("test_file.py").as_std_path(),
            "def test_function(): pass",
        );
        let path2 = env.create_file(
            test_dir.join("test_file2.py").as_std_path(),
            "def test_function(): pass",
        );

        let project = Project::new(env.cwd(), vec![path, path2]);
        let discoverer = Discoverer::new(&project);
        let (session, _) = discoverer.discover();
        assert_eq!(
            session.display(),
            StringPackage {
                modules: HashMap::new(),
                packages: HashMap::from([(
                    test_dir.strip_prefix(env.cwd()).unwrap().to_string(),
                    StringPackage {
                        modules: HashMap::from([
                            (
                                "test_file".to_string(),
                                StringModule {
                                    test_cases: HashSet::from(["test_function".to_string(),]),
                                    fixtures: HashSet::new(),
                                },
                            ),
                            (
                                "test_file2".to_string(),
                                StringModule {
                                    test_cases: HashSet::from(["test_function".to_string(),]),
                                    fixtures: HashSet::new(),
                                },
                            )
                        ]),
                        packages: HashMap::new(),
                    }
                )]),
            }
        );
        assert_eq!(session.total_test_cases(), 2);
    }

    #[test]
    fn test_discover_files_with_conftest_explicit_path() {
        let env = TestEnv::new();
        let test_dir = env.create_tests_dir();
        let conftest_path = env.create_file(
            test_dir.join("conftest.py").as_std_path(),
            "def test_function(): pass",
        );
        env.create_file(
            test_dir.join("test_file.py").as_std_path(),
            "def test_function2(): pass",
        );

        let project = Project::new(env.cwd(), vec![conftest_path]);
        let discoverer = Discoverer::new(&project);
        let (session, _) = discoverer.discover();

        assert_eq!(
            session.display(),
            StringPackage {
                modules: HashMap::new(),
                packages: HashMap::from([(
                    test_dir.strip_prefix(env.cwd()).unwrap().to_string(),
                    StringPackage {
                        modules: HashMap::from([(
                            "conftest".to_string(),
                            StringModule {
                                test_cases: HashSet::from(["test_function".to_string(),]),
                                fixtures: HashSet::new(),
                            },
                        )]),
                        packages: HashMap::new(),
                    }
                )]),
            }
        );
        assert_eq!(session.total_test_cases(), 1);
    }

    #[test]
    fn test_discover_files_with_conftest_parent_path_conftest_not_discovered() {
        let env = TestEnv::new();
        let test_dir = env.create_tests_dir();
        env.create_file(
            test_dir.join("conftest.py").as_std_path(),
            "def test_function(): pass",
        );
        env.create_file(
            test_dir.join("test_file.py").as_std_path(),
            "def test_function2(): pass",
        );

        let project = Project::new(env.cwd(), vec![test_dir.clone()]);
        let discoverer = Discoverer::new(&project);
        let (session, _) = discoverer.discover();

        assert_eq!(
            session.display(),
            StringPackage {
                modules: HashMap::new(),
                packages: HashMap::from([(
                    test_dir.strip_prefix(env.cwd()).unwrap().to_string(),
                    StringPackage {
                        modules: HashMap::from([(
                            "test_file".to_string(),
                            StringModule {
                                test_cases: HashSet::from(["test_function2".to_string(),]),
                                fixtures: HashSet::new(),
                            },
                        ),]),
                        packages: HashMap::new(),
                    }
                )]),
            }
        );
        assert_eq!(session.total_test_cases(), 1);
    }

    #[test]
    fn test_discover_files_with_cwd_path() {
        let env = TestEnv::new();
        let path = env.cwd();
        let test_dir = env.create_tests_dir();
        env.create_file(
            test_dir.join("test_file.py").as_std_path(),
            "def test_function(): pass",
        );

        let project = Project::new(env.cwd(), vec![path]);
        let discoverer = Discoverer::new(&project);
        let (session, _) = discoverer.discover();

        assert_eq!(
            session.display(),
            StringPackage {
                modules: HashMap::new(),
                packages: HashMap::from([(
                    test_dir.strip_prefix(env.cwd()).unwrap().to_string(),
                    StringPackage {
                        modules: HashMap::from([(
                            "test_file".to_string(),
                            StringModule {
                                test_cases: HashSet::from(["test_function".to_string(),]),
                                fixtures: HashSet::new(),
                            },
                        )]),
                        packages: HashMap::new(),
                    }
                )]),
            }
        );
        assert_eq!(session.total_test_cases(), 1);
    }

    #[test]
    fn test_discover_function_inside_function() {
        let env = TestEnv::new();
        let path = env.create_file(
            "test_file.py",
            "def test_function():
    def test_function2(): pass",
        );

        let project = Project::new(env.cwd(), vec![path]);
        let discoverer = Discoverer::new(&project);

        let (session, _) = discoverer.discover();

        assert_eq!(
            session.display(),
            StringPackage {
                modules: HashMap::from([(
                    "test_file".to_string(),
                    StringModule {
                        test_cases: HashSet::from(["test_function".to_string()]),
                        fixtures: HashSet::new(),
                    },
                )]),
                packages: HashMap::new(),
            }
        );
    }

    #[test]
    fn test_discover_fixture_in_same_file_in_root() {
        let env = TestEnv::new();

        let test_path = env.create_file(
            "test_1.py",
            r"
import karva
@karva.fixture(scope='function')
def x():
    return 1

def test_1(x): pass",
        );

        for path in [env.cwd(), test_path] {
            let project = Project::new(env.cwd().clone(), vec![path.clone()]);
            let (session, _) = Discoverer::new(&project).discover();
            assert_eq!(
                session.display(),
                StringPackage {
                    modules: HashMap::from([(
                        "test_1".to_string(),
                        StringModule {
                            test_cases: HashSet::from(["test_1".to_string(),]),
                            fixtures: HashSet::from([("x".to_string(), "function".to_string())]),
                        },
                    )]),
                    packages: HashMap::new(),
                },
                "{path}",
            );
        }
    }

    #[test]
    fn test_discover_fixture_in_same_file_in_tests_dir() {
        let env = TestEnv::new();
        let fixture = r"
import karva
@karva.fixture(scope='function')
def x():
    return 1
";

        let tests_dir = env.create_tests_dir();

        let test_path = env.create_file(
            tests_dir.join("test_1.py").as_std_path(),
            &format!("{fixture}def test_1(x): pass\n"),
        );

        for path in [env.cwd(), tests_dir.clone(), test_path] {
            let project = Project::new(env.cwd().clone(), vec![path.clone()]);
            let (session, _) = Discoverer::new(&project).discover();
            assert_eq!(
                session.display(),
                StringPackage {
                    modules: HashMap::new(),
                    packages: HashMap::from([(
                        tests_dir.strip_prefix(env.cwd()).unwrap().to_string(),
                        StringPackage {
                            modules: HashMap::from([(
                                "test_1".to_string(),
                                StringModule {
                                    test_cases: HashSet::from(["test_1".to_string(),]),
                                    fixtures: HashSet::from([(
                                        "x".to_string(),
                                        "function".to_string()
                                    )]),
                                },
                            )]),
                            packages: HashMap::new(),
                        }
                    )]),
                },
                "{path}"
            );
        }
    }

    #[test]
    fn test_discover_fixture_in_root_tests_in_tests_dir() {
        let env = TestEnv::new();
        let fixture = r"
import karva
@karva.fixture(scope='function')
def x():
    return 1
";

        let tests_dir = env.create_tests_dir();

        env.create_file("conftest.py", fixture);

        let test_path = env.create_file(
            tests_dir.join("test_1.py").as_std_path(),
            "def test_1(x): pass\n",
        );

        for path in [env.cwd(), tests_dir.clone(), test_path] {
            let project = Project::new(env.cwd().clone(), vec![path.clone()]);
            let (session, _) = Discoverer::new(&project).discover();

            assert_eq!(
                session.display(),
                StringPackage {
                    modules: HashMap::from([(
                        "conftest".to_string(),
                        StringModule {
                            test_cases: HashSet::new(),
                            fixtures: HashSet::from([("x".to_string(), "function".to_string())]),
                        },
                    )]),
                    packages: HashMap::from([(
                        tests_dir.strip_prefix(env.cwd()).unwrap().to_string(),
                        StringPackage {
                            modules: HashMap::from([(
                                "test_1".to_string(),
                                StringModule {
                                    test_cases: HashSet::from(["test_1".to_string(),]),
                                    fixtures: HashSet::new(),
                                },
                            )]),
                            packages: HashMap::new(),
                        }
                    )]),
                },
                "{path}"
            );
        }
    }

    #[test]
    fn test_discover_fixture_in_root_tests_in_nested_dir() {
        let env = TestEnv::new();
        let fixture_x = r"
import karva
@karva.fixture(scope='function')
def x():
    return 1
";

        env.create_file("conftest.py", fixture_x);

        let nested_dir = env.create_dir("nested_dir");

        let fixture_y = r"
import karva
@karva.fixture(scope='function')
def y(x):
    return 2
";

        env.create_file(nested_dir.join("conftest.py").as_std_path(), fixture_y);

        let more_nested_dir = nested_dir.join("more_nested_dir");

        let fixture_z = r"
import karva
@karva.fixture(scope='function')
def z(x, y):
    return 3
";

        env.create_file(more_nested_dir.join("conftest.py").as_std_path(), fixture_z);

        let even_more_nested_dir = more_nested_dir.join("even_more_nested_dir");

        let fixture_w = r"
import karva
@karva.fixture(scope='function')
def w(x, y, z):
    return 4
";

        env.create_file(
            even_more_nested_dir.join("conftest.py").as_std_path(),
            fixture_w,
        );

        let test_path = env.create_file(
            even_more_nested_dir.join("test_1.py").as_std_path(),
            "def test_1(x): pass\n",
        );

        for path in [
            env.cwd(),
            nested_dir.clone(),
            more_nested_dir.clone(),
            even_more_nested_dir.clone(),
            test_path,
        ] {
            let project = Project::new(env.cwd().clone(), vec![path.clone()]);
            let (session, _) = Discoverer::new(&project).discover();
            assert_eq!(
                session.display(),
                StringPackage {
                    modules: HashMap::from([(
                        "conftest".to_string(),
                        StringModule {
                            test_cases: HashSet::new(),
                            fixtures: HashSet::from([("x".to_string(), "function".to_string())]),
                        },
                    )]),
                    packages: HashMap::from([(
                        nested_dir
                            .clone()
                            .strip_prefix(env.cwd())
                            .unwrap()
                            .to_string(),
                        StringPackage {
                            modules: HashMap::from([(
                                "conftest".to_string(),
                                StringModule {
                                    test_cases: HashSet::new(),
                                    fixtures: HashSet::from([(
                                        "y".to_string(),
                                        "function".to_string()
                                    )]),
                                },
                            )]),
                            packages: HashMap::from([(
                                more_nested_dir
                                    .clone()
                                    .strip_prefix(&nested_dir)
                                    .unwrap()
                                    .to_string(),
                                StringPackage {
                                    modules: HashMap::from([(
                                        "conftest".to_string(),
                                        StringModule {
                                            test_cases: HashSet::new(),
                                            fixtures: HashSet::from([(
                                                "z".to_string(),
                                                "function".to_string()
                                            )]),
                                        },
                                    )]),
                                    packages: HashMap::from([(
                                        even_more_nested_dir
                                            .clone()
                                            .strip_prefix(&more_nested_dir)
                                            .unwrap()
                                            .to_string(),
                                        StringPackage {
                                            modules: HashMap::from([
                                                (
                                                    "conftest".to_string(),
                                                    StringModule {
                                                        test_cases: HashSet::new(),
                                                        fixtures: HashSet::from([(
                                                            "w".to_string(),
                                                            "function".to_string()
                                                        )]),
                                                    },
                                                ),
                                                (
                                                    "test_1".to_string(),
                                                    StringModule {
                                                        test_cases: HashSet::from([
                                                            "test_1".to_string(),
                                                        ]),
                                                        fixtures: HashSet::new(),
                                                    },
                                                )
                                            ]),
                                            packages: HashMap::new(),
                                        },
                                    )]),
                                },
                            )]),
                        },
                    ),]),
                },
                "{path}"
            );
        }
    }

    #[test]
    fn test_discover_multiple_test_paths() {
        let env = TestEnv::new();

        let tests_dir_1 = env.create_tests_dir();
        env.create_file(
            tests_dir_1.join("test_1.py").as_std_path(),
            "def test_1(): pass",
        );

        let tests_dir_2 = env.create_dir("tests2");
        env.create_file(
            tests_dir_2.join("test_2.py").as_std_path(),
            "def test_2(): pass",
        );

        let test_file_3 = env.create_file("test_3.py", "def test_3(): pass");

        let project = Project::new(
            env.cwd(),
            vec![tests_dir_1.clone(), tests_dir_2.clone(), test_file_3],
        );

        let (session, _) = Discoverer::new(&project).discover();

        assert_eq!(
            session.display(),
            StringPackage {
                modules: HashMap::from([(
                    "test_3".to_string(),
                    StringModule {
                        test_cases: HashSet::from(["test_3".to_string()]),
                        fixtures: HashSet::new(),
                    },
                ),]),
                packages: HashMap::from([
                    (
                        tests_dir_1.strip_prefix(env.cwd()).unwrap().to_string(),
                        StringPackage {
                            modules: HashMap::from([(
                                "test_1".to_string(),
                                StringModule {
                                    test_cases: HashSet::from(["test_1".to_string()]),
                                    fixtures: HashSet::new(),
                                },
                            ),]),
                            packages: HashMap::new(),
                        },
                    ),
                    (
                        tests_dir_2.strip_prefix(env.cwd()).unwrap().to_string(),
                        StringPackage {
                            modules: HashMap::from([(
                                "test_2".to_string(),
                                StringModule {
                                    test_cases: HashSet::from(["test_2".to_string()]),
                                    fixtures: HashSet::new(),
                                },
                            ),]),
                            packages: HashMap::new(),
                        },
                    ),
                ]),
            },
        );
    }

    #[test]
    fn test_discover_doubly_nested_with_conftest_middle_path() {
        let env = TestEnv::new();

        let fixture = r"
import karva
@karva.fixture(scope='function')
def root_fixture():
    return 'from_root'
";

        let tests_dir = env.create_tests_dir();
        env.create_file(tests_dir.join("conftest.py").as_std_path(), fixture);

        let middle_dir = env.create_dir(tests_dir.join("middle_dir").as_std_path());
        let deep_dir = env.create_dir(middle_dir.join("deep_dir").as_std_path());
        env.create_file(
            deep_dir.join("test_nested.py").as_std_path(),
            "def test_with_fixture(root_fixture): pass\ndef test_without_fixture(): pass",
        );

        let project = Project::new(env.cwd(), vec![middle_dir.clone()]);
        let (session, _) = Discoverer::new(&project).discover();

        assert_eq!(
            session.display(),
            StringPackage {
                modules: HashMap::new(),
                packages: HashMap::from([(
                    tests_dir.strip_prefix(env.cwd()).unwrap().to_string(),
                    StringPackage {
                        modules: HashMap::from([(
                            "conftest".to_string(),
                            StringModule {
                                test_cases: HashSet::new(),
                                fixtures: HashSet::from([(
                                    "root_fixture".to_string(),
                                    "function".to_string()
                                )]),
                            },
                        )]),
                        packages: HashMap::from([(
                            middle_dir.strip_prefix(tests_dir).unwrap().to_string(),
                            StringPackage {
                                modules: HashMap::new(),
                                packages: HashMap::from([(
                                    "deep_dir".to_string(),
                                    StringPackage {
                                        modules: HashMap::from([(
                                            "test_nested".to_string(),
                                            StringModule {
                                                test_cases: HashSet::from([
                                                    "test_with_fixture".to_string(),
                                                    "test_without_fixture".to_string(),
                                                ]),
                                                fixtures: HashSet::new(),
                                            },
                                        )]),
                                        packages: HashMap::new(),
                                    },
                                )]),
                            },
                        )]),
                    },
                )]),
            },
        );
        assert_eq!(session.total_test_cases(), 2);
    }

    #[test]
    #[ignore = "pytest is not supported in rust tests"]
    fn test_discover_pytest_fixture() {
        let env = TestEnv::new();

        let test_dir = env.create_tests_dir();

        let fixture = r"
import pytest

@pytest.fixture
def x():
    return 1
";

        env.create_file(test_dir.join("conftest.py").as_std_path(), fixture);

        env.create_file(
            test_dir.join("test_1.py").as_std_path(),
            "def test_1(x): pass",
        );

        let project = Project::new(env.cwd(), vec![test_dir.clone()]);
        let (session, _) = Discoverer::new(&project).discover();

        assert_eq!(
            session.display(),
            StringPackage {
                modules: HashMap::new(),
                packages: HashMap::from([(
                    test_dir.strip_prefix(env.cwd()).unwrap().to_string(),
                    StringPackage {
                        modules: HashMap::from([
                            (
                                "conftest".to_string(),
                                StringModule {
                                    test_cases: HashSet::new(),
                                    fixtures: HashSet::from([(
                                        "x".to_string(),
                                        "function".to_string()
                                    )]),
                                },
                            ),
                            (
                                "test_1".to_string(),
                                StringModule {
                                    test_cases: HashSet::from(["test_1".to_string()]),
                                    fixtures: HashSet::new(),
                                },
                            )
                        ]),
                        packages: HashMap::new(),
                    },
                )]),
            }
        );
    }

    #[test]
    fn test_discover_generator_fixture() {
        let env = TestEnv::new();

        let test_dir = env.create_tests_dir();

        let fixture = r"
import karva

@karva.fixture(scope='function')
def x():
    yield 1
";

        let conftest_path = env.create_file(test_dir.join("conftest.py").as_std_path(), fixture);

        env.create_file(
            test_dir.join("test_1.py").as_std_path(),
            "def test_1(x): pass",
        );

        let project = Project::new(env.cwd(), vec![test_dir.clone()]);
        let (session, _) = Discoverer::new(&project).discover();

        let tests_dir_name = test_dir.strip_prefix(env.cwd()).unwrap().to_string();

        assert_eq!(
            session.display(),
            StringPackage {
                modules: HashMap::new(),
                packages: HashMap::from([(
                    tests_dir_name,
                    StringPackage {
                        modules: HashMap::from([
                            (
                                "test_1".to_string(),
                                StringModule {
                                    test_cases: HashSet::from(["test_1".to_string()]),
                                    fixtures: HashSet::new(),
                                },
                            ),
                            (
                                "conftest".to_string(),
                                StringModule {
                                    test_cases: HashSet::new(),
                                    fixtures: HashSet::from([(
                                        "x".to_string(),
                                        "function".to_string()
                                    )]),
                                },
                            )
                        ]),
                        packages: HashMap::new(),
                    }
                )]),
            }
        );

        let test_1_module = session
            .packages()
            .get(&test_dir)
            .unwrap()
            .modules()
            .get(&conftest_path)
            .unwrap();

        let fixture = test_1_module.fixtures()[0];

        assert!(fixture.is_generator());
    }
}
