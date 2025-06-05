use std::collections::HashMap;

use ignore::WalkBuilder;
use indexmap::IndexSet;
use karva_project::{
    path::{PythonTestPath, SystemPathBuf},
    project::Project,
    utils::is_python_file,
};

use crate::discovery::{TestCase, function_definitions, module::Module};

pub struct Discoverer<'proj> {
    project: &'proj Project,
}

impl<'proj> Discoverer<'proj> {
    #[must_use]
    pub const fn new(project: &'proj Project) -> Self {
        Self { project }
    }

    #[must_use]
    pub fn discover(self) -> DiscoveredTests<'proj> {
        let mut all_discovered_tests: HashMap<Module<'proj>, IndexSet<TestCase<'proj>>> =
            HashMap::new();

        tracing::info!("Discovering tests...");

        for path in self.project.python_test_paths() {
            match path {
                Ok(path) => match path {
                    PythonTestPath::File(path) => {
                        self.discover_file(&path, &mut all_discovered_tests);
                    }
                    PythonTestPath::Directory(dir_path) => {
                        self.discover_directory(&dir_path, &mut all_discovered_tests);
                    }
                },
                Err(e) => {
                    tracing::warn!("Error discovering tests: {}", e);
                }
            }
        }

        let count: usize = all_discovered_tests
            .values()
            .map(indexmap::IndexSet::len)
            .sum();

        tracing::info!("Discovered {} tests", count);

        DiscoveredTests {
            tests: all_discovered_tests,
            count,
        }
    }

    fn discover_file(
        &self,
        path: &SystemPathBuf,
        all_discovered_tests: &mut HashMap<Module<'proj>, IndexSet<TestCase<'proj>>>,
    ) {
        let function_defs = function_definitions(path, self.project);
        if function_defs.is_empty() {
            return;
        }

        let module = Module::new(path, self.project);
        let mut test_cases = IndexSet::new();

        for function_def in function_defs {
            // Clone the module for TestCase creation since TestCase takes it by value
            let test_case = TestCase::new(module.clone(), function_def);
            test_cases.insert(test_case);
        }

        if !test_cases.is_empty() {
            all_discovered_tests.insert(module, test_cases);
        }
    }

    fn discover_directory(
        &self,
        path: &SystemPathBuf,
        all_discovered_tests: &mut HashMap<Module<'proj>, IndexSet<TestCase<'proj>>>,
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
            self.discover_file(&path, all_discovered_tests);
        }
    }
}

pub struct DiscoveredTests<'proj> {
    pub tests: HashMap<Module<'proj>, IndexSet<TestCase<'proj>>>,
    pub count: usize,
}

#[cfg(test)]
mod tests {
    use std::fs;

    use karva_project::project::ProjectOptions;
    use tempfile::TempDir;

    use super::*;

    struct TestEnv {
        temp_dir: TempDir,
    }

    impl TestEnv {
        fn new() -> Self {
            Self {
                temp_dir: TempDir::new().expect("Failed to create temp directory"),
            }
        }

        fn create_file(&self, name: &str, content: &str) -> String {
            let path = self.temp_dir.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&path, content).unwrap();
            path.display().to_string()
        }

        fn create_dir(&self, name: &str) -> String {
            let path = self.temp_dir.path().join(name);
            fs::create_dir_all(&path).unwrap();
            path.display().to_string()
        }

        fn cwd(&self) -> String {
            self.temp_dir.path().display().to_string()
        }
    }

    fn get_sorted_test_strings(
        discovered_tests: &HashMap<Module, IndexSet<TestCase>>,
    ) -> Vec<String> {
        let test_strings: Vec<Vec<String>> = discovered_tests
            .values()
            .map(|t| t.iter().map(ToString::to_string).collect())
            .collect();
        let mut flattened_test_strings: Vec<String> =
            test_strings.iter().flatten().cloned().collect();
        flattened_test_strings.sort();
        flattened_test_strings
    }

    #[test]
    fn test_discover_files() {
        let env = TestEnv::new();
        let path = env.create_file("test.py", "def test_function(): pass");

        let project = Project::new(SystemPathBuf::from(env.temp_dir.path()), vec![path]);
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();
        assert_eq!(
            get_sorted_test_strings(&discovered_tests.tests),
            vec!["test::test_function"]
        );
    }

    #[test]
    fn test_discover_files_with_directory() {
        let env = TestEnv::new();
        let path = env.create_dir("test_dir");

        env.create_file("test_dir/test_file1.py", "def test_function1(): pass");
        env.create_file("test_dir/test_file2.py", "def function2(): pass");

        let project = Project::new(SystemPathBuf::from(env.temp_dir.path()), vec![path]);
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();

        assert_eq!(
            get_sorted_test_strings(&discovered_tests.tests),
            vec!["test_dir.test_file1::test_function1"]
        );
    }

    #[test]
    fn test_discover_files_with_gitignore() {
        let env = TestEnv::new();
        let path = env.create_dir("tests");

        env.create_file(".gitignore", "tests/test_file2.py\n");
        env.create_file("tests/test_file1.py", "def test_function1(): pass");
        env.create_file("tests/test_file2.py", "def test_function2(): pass");

        let project = Project::new(SystemPathBuf::from(env.temp_dir.path()), vec![path]);
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();

        assert_eq!(
            get_sorted_test_strings(&discovered_tests.tests),
            vec!["tests.test_file1::test_function1"]
        );
    }

    #[test]
    fn test_discover_files_with_nested_directories() {
        let env = TestEnv::new();
        let path = env.create_dir("tests");
        env.create_dir("tests/nested");
        env.create_dir("tests/nested/deeper");

        env.create_file("tests/test_file1.py", "def test_function1(): pass");
        env.create_file("tests/nested/test_file2.py", "def test_function2(): pass");
        env.create_file(
            "tests/nested/deeper/test_file3.py",
            "def test_function3(): pass",
        );

        let project = Project::new(SystemPathBuf::from(env.temp_dir.path()), vec![path]);
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();

        assert_eq!(
            get_sorted_test_strings(&discovered_tests.tests),
            vec![
                "tests.nested.deeper.test_file3::test_function3",
                "tests.nested.test_file2::test_function2",
                "tests.test_file1::test_function1"
            ]
        );
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

        let project = Project::new(SystemPathBuf::from(env.temp_dir.path()), vec![path]);
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();

        assert_eq!(
            get_sorted_test_strings(&discovered_tests.tests),
            vec![
                "test_file::test_function1",
                "test_file::test_function2",
                "test_file::test_function3"
            ]
        );
    }

    #[test]
    fn test_discover_files_with_nonexistent_function() {
        let env = TestEnv::new();
        let path = env.create_file("test_file.py", "def test_function1(): pass");

        let project = Project::new(
            SystemPathBuf::from(env.temp_dir.path()),
            vec![format!("{path}::nonexistent_function")],
        );
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();

        assert!(get_sorted_test_strings(&discovered_tests.tests).is_empty());
    }

    #[test]
    fn test_discover_files_with_invalid_python() {
        let env = TestEnv::new();
        let path = env.create_file("test_file.py", "test_function1 = None");

        let project = Project::new(SystemPathBuf::from(env.temp_dir.path()), vec![path]);
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();

        assert!(get_sorted_test_strings(&discovered_tests.tests).is_empty());
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

        let project = Project::new(SystemPathBuf::from(env.temp_dir.path()), vec![path])
            .with_options(ProjectOptions {
                test_prefix: "check".to_string(),
                watch: false,
            });
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();

        assert_eq!(
            get_sorted_test_strings(&discovered_tests.tests),
            ["test_file::check_function1", "test_file::check_function2"]
        );
    }

    #[test]
    fn test_discover_files_with_multiple_paths() {
        let env = TestEnv::new();
        let file1 = env.create_file("test1.py", "def test_function1(): pass");
        let file2 = env.create_file("test2.py", "def test_function2(): pass");
        let dir = env.create_dir("tests");
        env.create_file("tests/test3.py", "def test_function3(): pass");

        let project = Project::new(
            SystemPathBuf::from(env.temp_dir.path()),
            vec![file1, file2, dir],
        );
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();

        assert_eq!(
            get_sorted_test_strings(&discovered_tests.tests),
            vec![
                "test1::test_function1",
                "test2::test_function2",
                "tests.test3::test_function3"
            ]
        );
    }

    #[test]
    fn test_tests_same_name_same_module_are_not_discovered_more_than_once() {
        let env = TestEnv::new();
        let path = env.create_file("tests/test_file.py", "def test_function(): pass");

        let project = Project::new(
            SystemPathBuf::from(env.temp_dir.path()),
            vec![format!("{}/tests", env.cwd()), path.clone(), path],
        );
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();
        assert_eq!(
            get_sorted_test_strings(&discovered_tests.tests),
            vec!["tests.test_file::test_function"]
        );
    }

    #[test]
    fn test_paths_shadowed_by_other_paths_are_not_discovered_twice() {
        let env = TestEnv::new();
        let path = env.create_file(
            "tests/test_file.py",
            "def test_function(): pass\ndef test_function2(): pass",
        );

        let project = Project::new(
            SystemPathBuf::from(env.temp_dir.path()),
            vec![path.clone(), path],
        );
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();
        assert_eq!(
            get_sorted_test_strings(&discovered_tests.tests),
            vec![
                "tests.test_file::test_function",
                "tests.test_file::test_function2"
            ]
        );
    }

    #[test]
    fn test_tests_same_name_different_module_are_discovered() {
        let env = TestEnv::new();
        let path = env.create_file("tests/test_file.py", "def test_function(): pass");
        let path2 = env.create_file("tests/test_file2.py", "def test_function(): pass");

        let project = Project::new(SystemPathBuf::from(env.temp_dir.path()), vec![path, path2]);
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();
        assert_eq!(
            get_sorted_test_strings(&discovered_tests.tests),
            vec![
                "tests.test_file2::test_function",
                "tests.test_file::test_function"
            ]
        );
    }
}
