use ignore::WalkBuilder;
use karva_project::{
    path::{PythonTestPath, SystemPathBuf},
    project::Project,
    utils::is_python_file,
};

use crate::{
    diagnostic::Diagnostic,
    discovery::discover,
    module::{Module, ModuleType},
    package::Package,
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

        tracing::info!("Discovering tests...");

        for path in self.project.python_test_paths() {
            match path {
                Ok(path) => match path {
                    PythonTestPath::File(path) => {
                        let test_cases = self.discover_test_file(&path, &mut discovery_diagnostics);
                        if let Some(module) = test_cases {
                            if path.parent().unwrap().as_std_path()
                                == self.project.cwd().as_std_path()
                            {
                                session_package.add_module(module);
                            } else {
                                let package_path = path.parent().unwrap().to_path_buf();
                                let mut package = Package::new(package_path, self.project);
                                package.add_module(module);
                                session_package.add_package(package);
                            }
                        }
                    }
                    PythonTestPath::Directory(dir_path) => {
                        let package =
                            self.discover_directory(&dir_path, &mut discovery_diagnostics);
                        if dir_path.as_std_path() == self.project.cwd().as_std_path() {
                            session_package.update(package);
                        } else {
                            session_package.add_package(package);
                        }
                    }
                },
                Err(e) => {
                    tracing::warn!("Error finding tests: {e}");
                }
            }
        }

        session_package.shrink();

        (session_package, discovery_diagnostics)
    }

    // Parse and run discovery on a single file
    fn discover_test_file(
        &self,
        path: &SystemPathBuf,
        discovery_diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<Module<'proj>> {
        tracing::debug!("Discovering file: {}", path);

        if !is_python_file(path) {
            return None;
        }

        let (discovered, diagnostics) = discover(path, self.project);

        discovery_diagnostics.extend(diagnostics);

        if discovered.is_empty() {
            return None;
        }

        let module_type = ModuleType::from_path(path);

        Some(Module::new(
            self.project,
            path,
            discovered.functions,
            discovered.fixtures,
            module_type,
        ))
    }

    fn discover_configuration_file(
        &self,
        path: &SystemPathBuf,
        discovery_diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<Module<'proj>> {
        tracing::debug!("Discovering configuration file: {}", path);

        if !is_python_file(path) {
            return None;
        }

        let (discovered, diagnostics) = discover(path, self.project);

        discovery_diagnostics.extend(diagnostics);

        if !discovered.functions.is_empty() {
            tracing::warn!("Found test functions in: {}", path);
        }

        Some(Module::new(
            self.project,
            path,
            Vec::new(),
            discovered.fixtures,
            ModuleType::Configuration,
        ))
    }

    // Parse and run discovery on a directory
    fn discover_directory(
        &self,
        path: &SystemPathBuf,
        discovery_diagnostics: &mut Vec<Diagnostic>,
    ) -> Package<'proj> {
        tracing::debug!("Discovering directory: {}", path);

        let mut package = Package::new(path.clone(), self.project);

        let walker = WalkBuilder::new(path.as_std_path())
            .max_depth(Some(1))
            .standard_filters(true)
            .require_git(false)
            .git_global(false)
            .parents(true)
            .build();

        for entry in walker {
            let Ok(entry) = entry else { continue };

            let current_path = SystemPathBuf::from(entry.path());

            if path == &current_path {
                continue;
            }

            if current_path.file_name() == Some("__pycache__") {
                continue;
            }

            match entry.file_type() {
                Some(file_type) if file_type.is_dir() => {
                    let subpackage = self.discover_directory(&current_path, discovery_diagnostics);
                    package.add_package(subpackage);
                }
                Some(file_type) if file_type.is_file() => {
                    match ModuleType::from_path(&current_path) {
                        ModuleType::Test => {
                            if let Some(module) =
                                self.discover_test_file(&current_path, discovery_diagnostics)
                            {
                                package.add_module(module);
                            }
                        }
                        ModuleType::Configuration => {
                            if let Some(module) = self
                                .discover_configuration_file(&current_path, discovery_diagnostics)
                            {
                                package.add_configuration_module(module);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        package
    }
}

#[cfg(test)]
mod tests {

    use std::collections::{HashMap, HashSet};

    use karva_project::{project::ProjectOptions, tests::TestEnv};

    use super::*;
    use crate::{module::StringModule, package::StringPackage};

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

        let project = Project::new(env.cwd(), vec![path]).with_options(ProjectOptions {
            test_prefix: "check".to_string(),
            ..Default::default()
        });
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
    fn test_discover_files_with_conftest_parent_path() {
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
                        modules: HashMap::from([
                            (
                                "test_file".to_string(),
                                StringModule {
                                    test_cases: HashSet::from(["test_function2".to_string(),]),
                                    fixtures: HashSet::new(),
                                },
                            ),
                            (
                                "conftest".to_string(),
                                StringModule {
                                    test_cases: HashSet::new(),
                                    fixtures: HashSet::new(),
                                },
                            )
                        ]),
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
}
