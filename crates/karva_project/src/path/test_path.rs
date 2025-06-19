use std::fmt::Formatter;

use crate::{path::SystemPathBuf, utils::is_python_file};

fn try_convert_to_py_path(path: &SystemPathBuf) -> Result<SystemPathBuf, TestPathError> {
    if path.exists() {
        return Ok(path.clone());
    }

    let path_with_py = SystemPathBuf::from(format!("{path}.py"));
    if path_with_py.exists() {
        return Ok(path_with_py);
    }

    let path_with_slash = SystemPathBuf::from(format!("{}.py", path.to_string().replace('.', "/")));
    if path_with_slash.exists() {
        return Ok(path_with_slash);
    }

    Err(TestPathError::NotFound(path.to_string()))
}

#[derive(Eq, PartialEq, Clone, Hash, PartialOrd, Ord, Debug)]
pub enum TestPath {
    File(SystemPathBuf),
    Directory(SystemPathBuf),
}

impl TestPath {
    pub fn new(value: &SystemPathBuf) -> Result<Self, TestPathError> {
        let path = try_convert_to_py_path(value)?;

        if path.is_file() {
            if is_python_file(&path) {
                Ok(Self::File(path))
            } else {
                Err(TestPathError::WrongFileExtension(path.to_string()))
            }
        } else if path.is_dir() {
            Ok(Self::Directory(path))
        } else {
            Err(TestPathError::InvalidPath(path.to_string()))
        }
    }

    #[must_use]
    pub const fn path(&self) -> &SystemPathBuf {
        match self {
            Self::File(path) | Self::Directory(path) => path,
        }
    }
}

#[derive(Debug)]
pub enum TestPathError {
    NotFound(String),
    WrongFileExtension(String),
    InvalidPath(String),
}

impl TestPathError {
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::NotFound(path) | Self::WrongFileExtension(path) | Self::InvalidPath(path) => path,
        }
    }
}

impl std::fmt::Display for TestPathError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(path) => write!(f, "Path `{path}` could not be found"),
            Self::WrongFileExtension(path) => {
                write!(f, "Path `{path}` has a wrong file extension")
            }
            Self::InvalidPath(path) => write!(f, "Path `{path}` is invalid"),
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::tests::TestEnv;

    #[test]
    fn test_python_file_exact_path() {
        let env = TestEnv::new();
        let path = env.create_file("test.py", "def test(): pass");

        let result = TestPath::new(&path);
        assert!(matches!(result, Ok(TestPath::File(_))));
    }

    #[test]
    fn test_python_file_auto_extension() {
        let env = TestEnv::new();
        env.create_file("test.py", "def test(): pass");
        let path_without_ext = env.temp_path("test");

        let result = TestPath::new(&path_without_ext);
        assert!(matches!(result, Ok(TestPath::File(_))));
    }

    #[test]
    fn test_directory_path() {
        let env = TestEnv::new();
        let path = env.create_dir("test_dir");

        let result = TestPath::new(&path);
        assert!(matches!(result, Ok(TestPath::Directory(_))));
    }

    #[test]
    fn test_file_not_found_exact_path() {
        let env = TestEnv::new();
        let non_existent_path = env.temp_path("non_existent.py");

        let result = TestPath::new(&non_existent_path);
        assert!(matches!(result, Err(TestPathError::NotFound(_))));
    }

    #[test]
    fn test_file_not_found_auto_extension() {
        let env = TestEnv::new();
        let non_existent_path = env.temp_path("non_existent");

        let result = TestPath::new(&non_existent_path);
        assert!(matches!(result, Err(TestPathError::NotFound(_))));
    }

    #[test]
    fn test_file_not_found_dotted_path() {
        let result = TestPath::new(&SystemPathBuf::from("non_existent.module"));
        assert!(matches!(result, Err(TestPathError::NotFound(_))));
    }

    #[test]
    fn test_invalid_path_with_extension() {
        let env = TestEnv::new();
        let path = env.create_file("path.txt", "def test(): pass");
        let result = TestPath::new(&path);
        assert!(matches!(result, Err(TestPathError::WrongFileExtension(_))));
    }

    #[test]
    fn test_wrong_file_extension() {
        let env = TestEnv::new();
        let path = env.create_file("test.rs", "fn test() {}");

        let result = TestPath::new(&path);
        assert!(matches!(result, Err(TestPathError::WrongFileExtension(_))));
    }

    #[test]
    fn test_path_that_exists_but_is_neither_file_nor_directory() {
        let env = TestEnv::new();
        let non_existent_path = env.temp_path("neither_file_nor_dir");

        let result = TestPath::new(&non_existent_path);
        assert!(matches!(result, Err(TestPathError::NotFound(_))));
    }

    #[test]
    fn test_file_and_auto_extension_both_exist() {
        let env = TestEnv::new();
        env.create_file("test", "not python");
        env.create_file("test.py", "def test(): pass");
        let base_path = env.temp_path("test");

        let result = TestPath::new(&base_path);
        assert!(matches!(result, Err(TestPathError::WrongFileExtension(_))));
    }

    #[test]
    fn test_try_convert_to_py_path_file() {
        let env = TestEnv::new();
        let env_path = env.create_file("test.py", "def test(): pass");

        let result = try_convert_to_py_path(&env.cwd().join("test"));
        if let Ok(path) = result {
            assert_eq!(path, env_path);
        } else {
            panic!("Expected Ok, got {result:?}");
        }
    }

    #[test]
    fn test_try_convert_to_py_path_file_slashes() {
        let env = TestEnv::new();
        let env_path = env.create_file("test/dir.py", "def test(): pass");

        let result = try_convert_to_py_path(&env.cwd().join("test/dir"));
        if let Ok(path) = result {
            assert_eq!(path, env_path);
        } else {
            panic!("Expected Ok, got {result:?}");
        }
    }

    #[test]
    fn test_try_convert_to_py_path_directory() {
        let env = TestEnv::new();
        let env_path = env.create_dir("test.dir");

        let result = try_convert_to_py_path(&env.cwd().join("test.dir"));
        if let Ok(path) = result {
            assert_eq!(path, env_path);
        } else {
            panic!("Expected Ok, got {result:?}");
        }
    }

    #[test]
    fn test_try_convert_to_py_path_not_found() {
        let env = TestEnv::new();
        let result = try_convert_to_py_path(&env.cwd().join("test/dir"));
        assert!(matches!(result, Err(TestPathError::NotFound(_))));
    }
}
