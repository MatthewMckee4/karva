use std::collections::{HashMap, HashSet};

use ignore::WalkBuilder;
use ruff_python_ast::StmtFunctionDef;

use crate::{
    discovery::{TestCase, function_definitions},
    path::{PythonTestPath, SystemPathBuf},
    project::Project,
    utils::{is_python_file, module_name},
};

pub struct Discoverer<'a> {
    project: &'a Project,
}

impl<'a> Discoverer<'a> {
    pub fn new(project: &'a Project) -> Self {
        Self { project }
    }

    pub fn discover(&self) -> HashMap<String, HashSet<TestCase>> {
        #[allow(clippy::mutable_key_type)]
        let mut discovered_tests: HashMap<String, HashSet<TestCase>> = HashMap::new();

        for path in self.project.paths() {
            discovered_tests.extend(self.discover_files(path));
        }

        discovered_tests
    }

    fn discover_files(&self, path: &PythonTestPath) -> HashMap<String, HashSet<TestCase>> {
        match path {
            PythonTestPath::File(path) => self.discover_file(path),
            PythonTestPath::Directory(dir_path) => self.discover_directory(dir_path),
            PythonTestPath::Function(path, function_name) => {
                if let Some(test_case) = self.discover_function(path, function_name) {
                    HashMap::from([(function_name.to_string(), HashSet::from([test_case]))])
                } else {
                    HashMap::new() // TODO: should this be an error?
                }
            }
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
                continue;
            }
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
        function_definitions(path.clone(), self.project)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

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

        fn create_file(&self, name: &str, content: &str) -> std::io::Result<SystemPathBuf> {
            let path = self.temp_dir.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, content)?;
            Ok(SystemPathBuf::from(path))
        }

        fn create_dir(&self, name: &str) -> std::io::Result<SystemPathBuf> {
            let path = self.temp_dir.path().join(name);
            fs::create_dir_all(&path)?;
            Ok(SystemPathBuf::from(path))
        }

        fn cwd(&self) -> SystemPathBuf {
            SystemPathBuf::from(self.temp_dir.path())
        }
    }

    #[test]
    fn test_discover_files() {
        let env = TestEnv::new();
        let path = env
            .create_file("test.py", "def test_function(): pass")
            .unwrap();
        let project = Project::new(
            SystemPathBuf::from(env.temp_dir.path()),
            vec![PythonTestPath::File(path)],
            "test".to_string(),
        );
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();
        assert_eq!(discovered_tests.len(), 1);
        assert!(
            discovered_tests["test"]
                .iter()
                .any(|t| t.function_definition().name.to_string() == "test_function")
        );
    }

    #[test]
    fn test_discover_files_with_directory() {
        let env = TestEnv::new();
        let path = env.create_dir("test_dir").unwrap();

        env.create_file("test_dir/test_file1.py", "def test_function1(): pass")
            .unwrap();
        env.create_file("test_dir/test_file2.py", "def function2(): pass")
            .unwrap();

        let project = Project::new(
            SystemPathBuf::from(env.temp_dir.path()),
            vec![PythonTestPath::Directory(path)],
            "test".to_string(),
        );
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();

        assert_eq!(discovered_tests.len(), 1);
        assert!(
            discovered_tests["test"]
                .iter()
                .any(|t| t.function_definition().name.to_string() == "test_function1")
        );
    }

    #[test]
    fn test_discover_files_with_gitignore() {
        let env = TestEnv::new();
        let path = env.create_dir("tests").unwrap();

        env.create_file(".gitignore", "tests/test_file2.py\n")
            .unwrap();

        env.create_file("tests/test_file1.py", "def test_function1(): pass")
            .unwrap();
        env.create_file("tests/test_file2.py", "def test_function2(): pass")
            .unwrap();

        let project = Project::new(
            SystemPathBuf::from(env.temp_dir.path()),
            vec![PythonTestPath::Directory(path)],
            "test".to_string(),
        );
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();

        assert_eq!(discovered_tests.len(), 1);
        assert!(
            discovered_tests["test"]
                .iter()
                .any(|t| t.function_definition().name.to_string() == "test_function1")
        );
    }

    #[test]
    fn test_discover_files_with_nested_directories() {
        let env = TestEnv::new();
        let path = env.create_dir("tests").unwrap();
        env.create_dir("tests/nested").unwrap();
        env.create_dir("tests/nested/deeper").unwrap();

        env.create_file("tests/test_file1.py", "def test_function1(): pass")
            .unwrap();
        env.create_file("tests/nested/test_file2.py", "def test_function2(): pass")
            .unwrap();
        env.create_file(
            "tests/nested/deeper/test_file3.py",
            "def test_function3(): pass",
        )
        .unwrap();

        let project = Project::new(
            SystemPathBuf::from(env.temp_dir.path()),
            vec![PythonTestPath::Directory(path)],
            "test".to_string(),
        );
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();

        assert_eq!(discovered_tests.len(), 3);
        let test_strings: Vec<String> = discovered_tests
            .values()
            .map(|t| t.iter().map(|t| t.to_string()).collect())
            .collect();
        assert!(
            test_strings
                .iter()
                .any(|s| s.ends_with("tests.test_file1::test_function1"))
        );
        assert!(
            test_strings
                .iter()
                .any(|s| s.ends_with("tests.nested.test_file2::test_function2"))
        );
        assert!(
            test_strings
                .iter()
                .any(|s| s.ends_with("tests.nested.deeper.test_file3::test_function3"))
        );
    }

    #[test]
    fn test_discover_files_with_multiple_test_functions() {
        let env = TestEnv::new();
        let path = env
            .create_file(
                "test_file.py",
                r#"
def test_function1(): pass
def test_function2(): pass
def test_function3(): pass
def not_a_test(): pass
"#,
            )
            .unwrap();

        let project = Project::new(
            SystemPathBuf::from(env.temp_dir.path()),
            vec![PythonTestPath::File(path)],
            "test".to_string(),
        );
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();

        assert_eq!(discovered_tests.len(), 3);
        let test_strings: Vec<String> = discovered_tests
            .values()
            .map(|t| t.iter().map(|t| t.to_string()).collect())
            .collect();
        assert!(test_strings.iter().any(|s| s.ends_with("test_function1")));
        assert!(test_strings.iter().any(|s| s.ends_with("test_function2")));
        assert!(test_strings.iter().any(|s| s.ends_with("test_function3")));
    }

    #[test]
    fn test_discover_files_with_specific_function() {
        let env = TestEnv::new();
        let path = env
            .create_file(
                "test_file.py",
                r#"
def test_function1(): pass
def test_function2(): pass
"#,
            )
            .unwrap();

        let project = Project::new(
            SystemPathBuf::from(env.temp_dir.path()),
            vec![PythonTestPath::Function(
                path.clone(),
                "test_function1".to_string(),
            )],
            "test".to_string(),
        );
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();

        assert_eq!(discovered_tests.len(), 1);
        assert!(
            discovered_tests["test"]
                .iter()
                .any(|t| t.function_definition().name.to_string() == "test_function1")
        );
    }

    #[test]
    fn test_discover_files_with_nonexistent_function() {
        let env = TestEnv::new();
        let path = env
            .create_file("test_file.py", "def test_function1(): pass")
            .unwrap();

        let project = Project::new(
            SystemPathBuf::from(env.temp_dir.path()),
            vec![PythonTestPath::Function(
                path,
                "nonexistent_function".to_string(),
            )],
            "test".to_string(),
        );
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();

        assert_eq!(discovered_tests.len(), 0);
    }

    #[test]
    fn test_discover_files_with_invalid_python() {
        let env = TestEnv::new();
        let path = env
            .create_file("test_file.py", "test_function1 = None")
            .unwrap();

        let project = Project::new(
            SystemPathBuf::from(env.temp_dir.path()),
            vec![PythonTestPath::File(path)],
            "test".to_string(),
        );
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();

        assert_eq!(discovered_tests.len(), 0);
    }

    #[test]
    fn test_discover_files_with_custom_test_prefix() {
        let env = TestEnv::new();
        let path = env
            .create_file(
                "test_file.py",
                r#"
def check_function1(): pass
def check_function2(): pass
def test_function(): pass
"#,
            )
            .unwrap();

        let project = Project::new(
            SystemPathBuf::from(env.temp_dir.path()),
            vec![PythonTestPath::File(path)],
            "check".to_string(),
        );
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();

        assert_eq!(discovered_tests.len(), 2);
        let test_strings: Vec<String> = discovered_tests
            .values()
            .map(|t| t.iter().map(|t| t.to_string()).collect())
            .collect();
        assert!(test_strings.iter().any(|s| s.ends_with("check_function1")));
        assert!(test_strings.iter().any(|s| s.ends_with("check_function2")));
    }

    #[test]
    fn test_discover_files_with_multiple_paths() {
        let env = TestEnv::new();
        let file1 = env
            .create_file("test1.py", "def test_function1(): pass")
            .unwrap();
        let file2 = env
            .create_file("test2.py", "def test_function2(): pass")
            .unwrap();
        let dir = env.create_dir("tests").unwrap();
        env.create_file("tests/test3.py", "def test_function3(): pass")
            .unwrap();

        let project = Project::new(
            SystemPathBuf::from(env.temp_dir.path()),
            vec![
                PythonTestPath::File(file1),
                PythonTestPath::File(file2),
                PythonTestPath::Directory(dir),
            ],
            "test".to_string(),
        );
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();

        assert_eq!(discovered_tests.len(), 3);
        let test_strings: Vec<String> = discovered_tests
            .values()
            .map(|t| t.iter().map(|t| t.to_string()).collect())
            .collect();
        assert!(test_strings.iter().any(|s| s.ends_with("test_function1")));
        assert!(test_strings.iter().any(|s| s.ends_with("test_function2")));
        assert!(test_strings.iter().any(|s| s.ends_with("test_function3")));
    }

    #[test]
    fn test_tests_same_name_same_module_are_not_discovered_more_than_once() {
        let env = TestEnv::new();
        let path = env
            .create_file("tests/test_file.py", "def test_function(): pass")
            .unwrap();

        let project = Project::new(
            SystemPathBuf::from(env.temp_dir.path()),
            vec![
                PythonTestPath::Directory(env.cwd().join("tests")),
                PythonTestPath::File(path.clone()),
                PythonTestPath::File(path.clone()),
                PythonTestPath::Function(path.clone(), "test_function".to_string()),
            ],
            "test".to_string(),
        );
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();
        assert_eq!(discovered_tests.len(), 1);
    }

    #[test]
    fn test_tests_same_name_different_module_are_discovered() {
        let env = TestEnv::new();
        let path = env
            .create_file("tests/test_file.py", "def test_function(): pass")
            .unwrap();
        let path2 = env
            .create_file("tests/test_file2.py", "def test_function(): pass")
            .unwrap();

        let project = Project::new(
            SystemPathBuf::from(env.temp_dir.path()),
            vec![
                PythonTestPath::File(path.clone()),
                PythonTestPath::File(path2.clone()),
            ],
            "test".to_string(),
        );
        let discoverer = Discoverer::new(&project);
        let discovered_tests = discoverer.discover();
        assert_eq!(discovered_tests.len(), 2);
    }
}
