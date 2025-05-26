use ignore::WalkBuilder;
use std::fmt::{self, Display};

use crate::path::{PythonTestPath, SystemPathBuf};
use crate::project::Project;
use crate::utils::{is_python_file, module_name};
use rustpython_parser::{Parse, ast};

pub struct Discoverer<'a> {
    project: &'a Project,
}

impl<'a> Discoverer<'a> {
    pub fn new(project: &'a Project) -> Self {
        Self { project }
    }

    pub fn discover(&self) -> Vec<DiscoveredTest> {
        let mut discovered_tests = Vec::new();

        for path in self.project.paths() {
            discovered_tests.extend(self.discover_files(path));
        }

        discovered_tests
    }

    fn discover_files(&self, path: &PythonTestPath) -> Vec<DiscoveredTest> {
        let mut discovered_tests = Vec::new();

        match path {
            PythonTestPath::File(path) => {
                discovered_tests.extend(self.test_functions_in_file(path).into_iter().map(
                    |function_name| {
                        DiscoveredTest::new(module_name(self.project.cwd(), path), function_name)
                    },
                ));
            }
            PythonTestPath::Directory(dir_path) => {
                let dir_path = dir_path.as_std_path().to_path_buf();
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
                        continue;
                    }
                    discovered_tests.extend(self.test_functions_in_file(&path).into_iter().map(
                        |function_name| {
                            DiscoveredTest::new(
                                module_name(self.project.cwd(), &path),
                                function_name,
                            )
                        },
                    ));
                }
            }
            PythonTestPath::Function(path, function_name) => {
                let discovered_tests_for_file = self.test_functions_in_file(path);
                if discovered_tests_for_file.contains(function_name) {
                    discovered_tests.push(DiscoveredTest::new(
                        module_name(self.project.cwd(), path),
                        function_name.clone(),
                    ));
                }
            }
        }

        discovered_tests
    }

    fn test_functions_in_file(&self, path: &SystemPathBuf) -> Vec<String> {
        let mut discovered_tests = Vec::new();
        let source = std::fs::read_to_string(path.as_std_path()).unwrap();
        let program = ast::Suite::parse(&source, "<embedded>");

        if let Ok(program) = program {
            for stmt in program {
                if let ast::Stmt::FunctionDef(ast::StmtFunctionDef { name, .. }) = stmt {
                    if name.to_string().starts_with(self.project.test_prefix()) {
                        discovered_tests.push(name.to_string());
                    }
                }
            }
        }

        discovered_tests
    }
}

#[derive(Debug, Clone)]
pub struct DiscoveredTest {
    module: String,
    function_name: String,
}

impl DiscoveredTest {
    pub fn new(module: String, function_name: String) -> Self {
        Self {
            module,
            function_name,
        }
    }

    pub fn module(&self) -> &String {
        &self.module
    }

    pub fn function_name(&self) -> &String {
        &self.function_name
    }
}

impl Display for DiscoveredTest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.module, self.function_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

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
            discovered_tests[0]
                .to_string()
                .ends_with("test::test_function")
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
            discovered_tests[0]
                .to_string()
                .ends_with("test_dir.test_file1::test_function1")
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
            discovered_tests[0]
                .to_string()
                .ends_with("tests.test_file1::test_function1")
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
        let test_strings: Vec<String> = discovered_tests.iter().map(|t| t.to_string()).collect();
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
        let test_strings: Vec<String> = discovered_tests.iter().map(|t| t.to_string()).collect();
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
        assert!(discovered_tests[0].to_string().ends_with("test_function1"));
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
            .create_file(
                "test_file.py",
                "def test_function1(): pass\ninvalid python syntax",
            )
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
        let test_strings: Vec<String> = discovered_tests.iter().map(|t| t.to_string()).collect();
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
        let test_strings: Vec<String> = discovered_tests.iter().map(|t| t.to_string()).collect();
        assert!(test_strings.iter().any(|s| s.ends_with("test_function1")));
        assert!(test_strings.iter().any(|s| s.ends_with("test_function2")));
        assert!(test_strings.iter().any(|s| s.ends_with("test_function3")));
    }
}
