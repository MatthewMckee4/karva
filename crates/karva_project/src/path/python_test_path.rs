use std::fmt::Formatter;

use crate::{path::SystemPathBuf, utils::is_python_file};

fn try_convert_to_py_path(path: &str) -> Result<SystemPathBuf, PythonTestPathError> {
    let file_path = SystemPathBuf::from(path);
    if file_path.exists() {
        return Ok(file_path);
    }

    let path_with_py = SystemPathBuf::from(format!("{path}.py"));
    if path_with_py.exists() {
        return Ok(path_with_py);
    }

    let path_with_slash = SystemPathBuf::from(format!("{}.py", path.replace('.', "/")));
    if path_with_slash.exists() {
        return Ok(path_with_slash);
    }

    Err(PythonTestPathError::NotFound(file_path.to_string()))
}

#[derive(Eq, PartialEq, Clone, Hash, PartialOrd, Ord)]
pub enum PythonTestPath {
    File(SystemPathBuf),
    Directory(SystemPathBuf),
}

impl PythonTestPath {
    pub fn new(value: impl AsRef<str>) -> Result<Self, PythonTestPathError> {
        let value = value.as_ref();

        let path = try_convert_to_py_path(value)?;

        if path.is_file() {
            if is_python_file(&path) {
                Ok(Self::File(path))
            } else {
                Err(PythonTestPathError::WrongFileExtension(path.to_string()))
            }
        } else if path.is_dir() {
            Ok(Self::Directory(path))
        } else {
            unreachable!("Path `{path}` is neither a file nor a directory")
        }
    }
}

#[derive(Debug)]
pub enum PythonTestPathError {
    NotFound(String),
    WrongFileExtension(String),
    InvalidPath(String),
}

impl std::fmt::Display for PythonTestPathError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(path) => write!(f, "Path `{path}` could not be found"),
            Self::WrongFileExtension(path) => {
                write!(f, "Path `{path}` has a wrong file extension")
            }
            Self::InvalidPath(path) => write!(f, "Path `{path}` is not a valid path"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    struct TestEnv {
        pub temp_dir: TempDir,
    }

    impl TestEnv {
        fn new() -> Self {
            Self {
                temp_dir: TempDir::new().expect("Failed to create temp directory"),
            }
        }

        fn create_test_file(&self, name: &str, content: &str) -> std::io::Result<String> {
            let path = self.temp_dir.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, content)?;
            Ok(path.display().to_string())
        }

        fn create_test_dir(&self, name: &str) -> std::io::Result<String> {
            let path = self.temp_dir.path().join(name);
            fs::create_dir_all(&path)?;
            Ok(path.display().to_string())
        }

        fn temp_path(&self, name: &str) -> String {
            self.temp_dir.path().join(name).display().to_string()
        }
    }

    #[test]
    fn test_python_file_exact_path() -> std::io::Result<()> {
        let env = TestEnv::new();
        let path = env.create_test_file("test.py", "def test(): pass")?;

        let result = PythonTestPath::new(path);
        assert!(matches!(result, Ok(PythonTestPath::File(_))));
        Ok(())
    }

    #[test]
    fn test_python_file_auto_extension() -> std::io::Result<()> {
        let env = TestEnv::new();
        env.create_test_file("test.py", "def test(): pass")?;
        let path_without_ext = env.temp_path("test");

        let result = PythonTestPath::new(path_without_ext);
        assert!(matches!(result, Ok(PythonTestPath::File(_))));
        Ok(())
    }

    #[test]
    fn test_python_file_dotted_path() -> std::io::Result<()> {
        let env = TestEnv::new();
        env.create_test_file("module/submodule.py", "def test(): pass")?;

        let temp_dir = env.temp_dir.path();
        let original_dir = std::env::current_dir()?;
        std::env::set_current_dir(temp_dir)?;

        let result = PythonTestPath::new("module.submodule");

        std::env::set_current_dir(original_dir)?;

        assert!(matches!(result, Ok(PythonTestPath::File(_))));
        Ok(())
    }

    #[test]
    fn test_directory_path() -> std::io::Result<()> {
        let env = TestEnv::new();
        let path = env.create_test_dir("test_dir")?;

        let result = PythonTestPath::new(path);
        assert!(matches!(result, Ok(PythonTestPath::Directory(_))));
        Ok(())
    }

    #[test]
    fn test_file_not_found_exact_path() {
        let env = TestEnv::new();
        let non_existent_path = env.temp_path("non_existent.py");

        let result = PythonTestPath::new(non_existent_path);
        assert!(matches!(result, Err(PythonTestPathError::NotFound(_))));
    }

    #[test]
    fn test_file_not_found_auto_extension() {
        let env = TestEnv::new();
        let non_existent_path = env.temp_path("non_existent");

        let result = PythonTestPath::new(non_existent_path);
        assert!(matches!(result, Err(PythonTestPathError::NotFound(_))));
    }

    #[test]
    fn test_file_not_found_dotted_path() {
        let result = PythonTestPath::new("non_existent.module");
        assert!(matches!(result, Err(PythonTestPathError::NotFound(_))));
    }

    #[test]
    fn test_wrong_file_extension() -> std::io::Result<()> {
        let env = TestEnv::new();
        let path = env.create_test_file("test.rs", "fn test() {}")?;

        let result = PythonTestPath::new(path);
        assert!(matches!(
            result,
            Err(PythonTestPathError::WrongFileExtension(_))
        ));
        Ok(())
    }

    #[test]
    fn test_path_that_exists_but_is_neither_file_nor_directory() {
        let env = TestEnv::new();
        let non_existent_path = env.temp_path("neither_file_nor_dir");

        let result = PythonTestPath::new(non_existent_path);
        assert!(matches!(result, Err(PythonTestPathError::NotFound(_))));
    }

    #[test]
    fn test_file_and_auto_extension_both_exist() -> std::io::Result<()> {
        let env = TestEnv::new();
        env.create_test_file("test", "not python")?;
        env.create_test_file("test.py", "def test(): pass")?;
        let base_path = env.temp_path("test");

        let result = PythonTestPath::new(base_path);
        assert!(matches!(
            result,
            Err(PythonTestPathError::WrongFileExtension(_))
        ));
        Ok(())
    }
}
