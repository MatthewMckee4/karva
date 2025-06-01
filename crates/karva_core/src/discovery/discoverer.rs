use std::collections::{HashMap, HashSet};

use ignore::WalkBuilder;
use karva_project::{
    path::{PythonTestPath, SystemPathBuf},
    project::Project,
    utils::{is_python_file, module_name},
};
use ruff_python_ast::StmtFunctionDef;

use crate::{
    diagnostic::DiscoveryDiagnosticWriter,
    discovery::{TestCase, function_definitions},
};

pub struct Discoverer<'a> {
    project: &'a Project,
    diagnostic_writer: &'a dyn DiscoveryDiagnosticWriter,
}

impl<'a> Discoverer<'a> {
    #[must_use]
    pub const fn new(
        project: &'a Project,
        diagnostic_writer: &'a dyn DiscoveryDiagnosticWriter,
    ) -> Self {
        Self {
            project,
            diagnostic_writer,
        }
    }

    #[must_use]
    pub fn discover(&self) -> HashMap<String, HashSet<TestCase>> {
        #[allow(clippy::mutable_key_type)]
        let mut discovered_tests: HashMap<String, HashSet<TestCase>> = HashMap::new();

        self.diagnostic_writer.discovery_started();

        for path in self.project.python_test_paths() {
            discovered_tests.extend(self.discover_files(&path.unwrap()));
        }

        self.diagnostic_writer.discovery_completed(
            discovered_tests
                .values()
                .map(std::collections::HashSet::len)
                .sum(),
        );

        discovered_tests
    }

    fn discover_files(&self, path: &PythonTestPath) -> HashMap<String, HashSet<TestCase>> {
        match path {
            PythonTestPath::File(path) => self.discover_file(path),
            PythonTestPath::Directory(dir_path) => self.discover_directory(dir_path),
            PythonTestPath::Function(path, function_name) => self
                .discover_function(path, function_name)
                .map_or_else(HashMap::new, |test_case| {
                    HashMap::from([(test_case.module().to_string(), HashSet::from([test_case]))])
                }),
        }
    }

    fn discover_file(&self, path: &SystemPathBuf) -> HashMap<String, HashSet<TestCase>> {
        let mut discovered_tests: HashMap<String, HashSet<TestCase>> = HashMap::new();
        let module_name = module_name(self.project.cwd(), path);

        for function_name in self.test_functions_in_file(path) {
            let test_case = TestCase::new(module_name.clone(), function_name);
            discovered_tests
                .entry(module_name.clone())
                .or_default()
                .insert(test_case);
        }
        discovered_tests
    }

    fn discover_directory(&self, path: &SystemPathBuf) -> HashMap<String, HashSet<TestCase>> {
        let dir_path = path.as_std_path().to_path_buf();

        let walker = WalkBuilder::new(self.project.cwd().as_std_path())
            .standard_filters(true)
            .require_git(false)
            .parents(false)
            .filter_entry(move |entry| entry.path().starts_with(&dir_path))
            .build();

        let mut discovered_tests: HashMap<String, HashSet<TestCase>> = HashMap::new();
        for entry in walker.flatten() {
            let entry_path = entry.path();
            let path = SystemPathBuf::from(entry_path);

            if !is_python_file(&path) {
                tracing::debug!("Skipping non-python file: {}", entry.path().display());
                continue;
            }
            tracing::debug!("Discovering file: {}", entry.path().display());
            let discovered_tests_for_file = self.discover_file(&path);
            for (module_name, test_cases) in discovered_tests_for_file {
                discovered_tests
                    .entry(module_name)
                    .or_default()
                    .extend(test_cases);
            }
        }

        discovered_tests
    }

    fn discover_function(&self, path: &SystemPathBuf, function_name: &str) -> Option<TestCase> {
        let discovered_tests_for_file = self.test_functions_in_file(path);
        for function_def in discovered_tests_for_file {
            if function_def.name == *function_name {
                return Some(TestCase::new(
                    module_name(self.project.cwd(), path),
                    function_def,
                ));
            }
        }
        None
    }

    fn test_functions_in_file(&self, path: &SystemPathBuf) -> Vec<StmtFunctionDef> {
        function_definitions(path, self.project)
    }
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
        discovered_tests: &HashMap<String, HashSet<TestCase>>,
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

    struct TestDiagnosticWriter {}

    impl crate::diagnostic::TestCaseDiagnosticWriter for TestDiagnosticWriter {
        fn test_started(&self, _test_name: &str, _file_path: &str) {
            // No-op for tests
        }

        fn test_completed(&mut self, _test: &crate::test_result::TestResult) {
            // No-op for tests
        }

        fn error(&self, _error: &str) {
            // No-op for tests
        }

        fn display_diagnostics(&self, _run_diagnostics: &crate::runner::RunDiagnostics) {
            // No-op for tests
        }
    }

    impl DiscoveryDiagnosticWriter for TestDiagnosticWriter {
        fn discovery_started(&self) {
            // No-op for tests
        }

        fn discovery_completed(&self, _count: usize) {
            // No-op for tests
        }
    }

    fn get_test_diagnostic_writer() -> &'static dyn DiscoveryDiagnosticWriter {
        &TestDiagnosticWriter {}
    }

    #[test]
    fn test_discover_files() {
        let env = TestEnv::new();
        let path = env.create_file("test.py", "def test_function(): pass");

        let project = Project::new(SystemPathBuf::from(env.temp_dir.path()), vec![path]);
        let discoverer = Discoverer::new(&project, get_test_diagnostic_writer());
        let discovered_tests = discoverer.discover();
        assert_eq!(
            get_sorted_test_strings(&discovered_tests),
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
        let discoverer = Discoverer::new(&project, get_test_diagnostic_writer());
        let discovered_tests = discoverer.discover();

        assert_eq!(
            get_sorted_test_strings(&discovered_tests),
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
        let discoverer = Discoverer::new(&project, get_test_diagnostic_writer());
        let discovered_tests = discoverer.discover();

        assert_eq!(
            get_sorted_test_strings(&discovered_tests),
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
        let discoverer = Discoverer::new(&project, get_test_diagnostic_writer());
        let discovered_tests = discoverer.discover();

        assert_eq!(
            get_sorted_test_strings(&discovered_tests),
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
        let discoverer = Discoverer::new(&project, get_test_diagnostic_writer());
        let discovered_tests = discoverer.discover();

        assert_eq!(
            get_sorted_test_strings(&discovered_tests),
            vec![
                "test_file::test_function1",
                "test_file::test_function2",
                "test_file::test_function3"
            ]
        );
    }

    #[test]
    fn test_discover_files_with_specific_function() {
        let env = TestEnv::new();
        let path = env.create_file(
            "test_file.py",
            r"
def test_function1(): pass
def test_function2(): pass
",
        );

        let project = Project::new(
            SystemPathBuf::from(env.temp_dir.path()),
            vec![format!("{path}::test_function1")],
        );
        let discoverer = Discoverer::new(&project, get_test_diagnostic_writer());
        let discovered_tests = discoverer.discover();

        assert_eq!(
            get_sorted_test_strings(&discovered_tests),
            vec!["test_file::test_function1"]
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
        let discoverer = Discoverer::new(&project, get_test_diagnostic_writer());
        let discovered_tests = discoverer.discover();

        assert!(get_sorted_test_strings(&discovered_tests).is_empty());
    }

    #[test]
    fn test_discover_files_with_invalid_python() {
        let env = TestEnv::new();
        let path = env.create_file("test_file.py", "test_function1 = None");

        let project = Project::new(SystemPathBuf::from(env.temp_dir.path()), vec![path]);
        let discoverer = Discoverer::new(&project, get_test_diagnostic_writer());
        let discovered_tests = discoverer.discover();

        assert!(get_sorted_test_strings(&discovered_tests).is_empty());
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
        let discoverer = Discoverer::new(&project, get_test_diagnostic_writer());
        let discovered_tests = discoverer.discover();

        assert_eq!(
            get_sorted_test_strings(&discovered_tests),
            vec!["test_file::check_function1", "test_file::check_function2",]
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
        let discoverer = Discoverer::new(&project, get_test_diagnostic_writer());
        let discovered_tests = discoverer.discover();

        assert_eq!(
            get_sorted_test_strings(&discovered_tests),
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
            vec![
                format!("{}/tests", env.cwd()),
                path.clone(),
                path.clone(),
                format!("{path}::test_function"),
            ],
        );
        let discoverer = Discoverer::new(&project, get_test_diagnostic_writer());
        let discovered_tests = discoverer.discover();
        assert_eq!(
            get_sorted_test_strings(&discovered_tests),
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
            vec![format!("{path}::test_function"), path],
        );
        let discoverer = Discoverer::new(&project, get_test_diagnostic_writer());
        let discovered_tests = discoverer.discover();
        assert_eq!(
            get_sorted_test_strings(&discovered_tests),
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
        let discoverer = Discoverer::new(&project, get_test_diagnostic_writer());
        let discovered_tests = discoverer.discover();
        assert_eq!(
            get_sorted_test_strings(&discovered_tests),
            vec![
                "tests.test_file2::test_function",
                "tests.test_file::test_function"
            ]
        );
    }
}
