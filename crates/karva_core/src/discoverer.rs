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
            PythonTestPath::Directory(path) => {
                println!("Walking directory: {:?}", path.as_std_path());
                let walker = WalkBuilder::new(path.as_std_path())
                    .standard_filters(true)
                    .build();

                for result in walker {
                    match result {
                        Ok(entry) => {
                            println!("Found file: {:?}", entry.path());
                            let path = SystemPathBuf::from(entry.path());
                            if is_python_file(&path) {
                                discovered_tests.extend(
                                    self.test_functions_in_file(&path).into_iter().map(
                                        |function_name| {
                                            DiscoveredTest::new(
                                                module_name(self.project.cwd(), &path),
                                                function_name,
                                            )
                                        },
                                    ),
                                );
                            }
                        }
                        Err(e) => {
                            println!("Error walking directory: {:?}", e);
                            continue;
                        }
                    }
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

        fn create_test_file(&self, name: &str, content: &str) -> std::io::Result<SystemPathBuf> {
            let path = self.temp_dir.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, content)?;
            Ok(SystemPathBuf::from(path))
        }

        fn create_test_dir(&self, name: &str) -> std::io::Result<SystemPathBuf> {
            let path = self.temp_dir.path().join(name);
            fs::create_dir_all(&path)?;
            Ok(SystemPathBuf::from(path))
        }
    }

    #[test]
    fn test_discover_files() {
        let env = TestEnv::new();
        let path = env
            .create_test_file("test.py", "def test_function(): pass")
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
                .ends_with("test.py::test_function")
        );
    }

    #[test]
    fn test_discover_files_with_directory() {
        let env = TestEnv::new();
        let path = env.create_test_dir("test_dir").unwrap();

        env.create_test_file("test_dir/test_file1.py", "def test_function1(): pass")
            .unwrap();
        env.create_test_file("test_dir/test_file2.py", "def function2(): pass")
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
                .ends_with("test_file1.py::test_function1")
        );
    }

    #[test]
    fn test_discover_files_with_gitignore() {
        let env = TestEnv::new();
        let path = env.create_test_dir("tests").unwrap();

        env.create_test_file(".gitignore", "test_file2.py\n")
            .unwrap();

        env.create_test_file("tests/test_file1.py", "def test_function1(): pass")
            .unwrap();
        env.create_test_file("tests/test_file2.py", "def test_function2(): pass")
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
                .ends_with("test_file1.py::test_function1")
        );
    }
}
