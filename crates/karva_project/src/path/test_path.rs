use std::{
    fmt::Formatter,
    path::{Path, PathBuf},
};

use crate::utils::is_python_file;

fn try_convert_to_py_path(path: &Path) -> Result<PathBuf, TestPathError> {
    if path.exists() {
        return Ok(path.to_path_buf());
    }

    let path_with_py = PathBuf::from(format!("{}.py", path.display()));
    if path_with_py.exists() {
        return Ok(path_with_py);
    }

    let path_with_slash = PathBuf::from(format!(
        "{}.py",
        path.display().to_string().replace('.', "/")
    ));
    if path_with_slash.exists() {
        return Ok(path_with_slash);
    }

    Err(TestPathError::NotFound(path.to_path_buf()))
}

#[derive(Eq, PartialEq, Clone, Hash, PartialOrd, Ord, Debug)]
pub enum TestPath {
    /// A file containing test functions.
    ///
    /// Some examples are:
    /// - `test_file.py`
    /// - `test_file`
    File(PathBuf),

    /// A directory containing test files.
    ///
    /// Some examples are:
    /// - `tests/`
    Directory(PathBuf),

    /// A function in a file containing test functions.
    ///
    /// Some examples are:
    /// - `test_file.py::test_function`
    /// - `test_file::test_function`
    Function {
        path: PathBuf,
        function_name: String,
    },
}

impl TestPath {
    pub(crate) fn new(value: &str) -> Result<Self, TestPathError> {
        if let Some(separator_pos) = value.rfind("::") {
            let file_part = &value[..separator_pos];
            let function_name = &value[separator_pos + 2..];

            if function_name.is_empty() {
                return Err(TestPathError::MissingFunctionName(PathBuf::from(file_part)));
            }

            let file_path = PathBuf::from(file_part);
            let path = try_convert_to_py_path(&file_path)?;

            if path.is_file() {
                if is_python_file(&path) {
                    Ok(Self::Function {
                        path,
                        function_name: function_name.to_string(),
                    })
                } else {
                    Err(TestPathError::WrongFileExtension(path))
                }
            } else {
                Err(TestPathError::InvalidPath(path))
            }
        } else {
            // Original file/directory logic
            let value = PathBuf::from(value);
            let path = try_convert_to_py_path(&value)?;

            if path.is_file() {
                if is_python_file(&path) {
                    Ok(Self::File(path))
                } else {
                    Err(TestPathError::WrongFileExtension(path))
                }
            } else if path.is_dir() {
                Ok(Self::Directory(path))
            } else {
                Err(TestPathError::InvalidPath(path))
            }
        }
    }

    #[must_use]
    pub const fn path(&self) -> &PathBuf {
        match self {
            Self::File(path) | Self::Directory(path) | Self::Function { path, .. } => path,
        }
    }
}

#[derive(Debug)]
pub enum TestPathError {
    NotFound(PathBuf),
    WrongFileExtension(PathBuf),
    InvalidPath(PathBuf),
    MissingFunctionName(PathBuf),
}

impl TestPathError {
    #[must_use]
    pub const fn path(&self) -> &PathBuf {
        match self {
            Self::NotFound(path)
            | Self::WrongFileExtension(path)
            | Self::InvalidPath(path)
            | Self::MissingFunctionName(path) => path,
        }
    }
}

impl std::fmt::Display for TestPathError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(path) => write!(f, "Path `{}` could not be found", path.display()),
            Self::WrongFileExtension(path) => {
                write!(f, "Path `{}` has a wrong file extension", path.display())
            }
            Self::InvalidPath(path) => write!(f, "Path `{}` is invalid", path.display()),
            Self::MissingFunctionName(path) => {
                write!(f, "Invalid function specification: `{}::`", path.display())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use karva_test::TestContext;

    use super::*;

    #[test]
    fn test_python_file_exact_path() {
        let env = TestContext::new();
        let path = env.create_file("test.py", "def test(): pass");

        let result = TestPath::new(&path.display().to_string());
        assert!(matches!(result, Ok(TestPath::File(_))));
    }

    #[test]
    fn test_python_file_auto_extension() {
        let env = TestContext::new();
        env.create_file("test.py", "def test(): pass");
        let path_without_ext = env.temp_path("test");

        let result = TestPath::new(&path_without_ext.display().to_string());
        assert!(matches!(result, Ok(TestPath::File(_))));
    }

    #[test]
    fn test_directory_path() {
        let env = TestContext::new();
        let path = env.create_dir("test_dir");

        let result = TestPath::new(&path.display().to_string());
        assert!(matches!(result, Ok(TestPath::Directory(_))));
    }

    #[test]
    fn test_file_not_found_exact_path() {
        let env = TestContext::new();
        let non_existent_path = env.temp_path("non_existent.py");

        let result = TestPath::new(&non_existent_path.display().to_string());
        assert!(matches!(result, Err(TestPathError::NotFound(_))));
    }

    #[test]
    fn test_file_not_found_auto_extension() {
        let env = TestContext::new();
        let non_existent_path = env.temp_path("non_existent");

        let result = TestPath::new(&non_existent_path.display().to_string());
        assert!(matches!(result, Err(TestPathError::NotFound(_))));
    }

    #[test]
    fn test_file_not_found_dotted_path() {
        let result = TestPath::new("non_existent.module");
        assert!(matches!(result, Err(TestPathError::NotFound(_))));
    }

    #[test]
    fn test_invalid_path_with_extension() {
        let env = TestContext::new();
        let path = env.create_file("path.txt", "def test(): pass");
        let result = TestPath::new(&path.display().to_string());
        assert!(matches!(result, Err(TestPathError::WrongFileExtension(_))));
    }

    #[test]
    fn test_wrong_file_extension() {
        let env = TestContext::new();
        let path = env.create_file("test.rs", "fn test() {}");

        let result = TestPath::new(&path.display().to_string());
        assert!(matches!(result, Err(TestPathError::WrongFileExtension(_))));
    }

    #[test]
    fn test_path_that_exists_but_is_neither_file_nor_directory() {
        let env = TestContext::new();
        let non_existent_path = env.temp_path("neither_file_nor_dir");

        let result = TestPath::new(&non_existent_path.display().to_string());
        assert!(matches!(result, Err(TestPathError::NotFound(_))));
    }

    #[test]
    fn test_file_and_auto_extension_both_exist() {
        let env = TestContext::new();
        env.create_file("test", "not python");
        env.create_file("test.py", "def test(): pass");
        let base_path = env.temp_path("test");

        let result = TestPath::new(&base_path.display().to_string());
        assert!(matches!(result, Err(TestPathError::WrongFileExtension(_))));
    }

    #[test]
    fn test_function_specification() {
        let env = TestContext::new();
        let path = env.create_file("test.py", "def test_function(): pass");

        let function_spec = format!("{}::test_function", path.display());
        let result = TestPath::new(&function_spec);

        if let Ok(TestPath::Function {
            path: result_path,
            function_name,
        }) = result
        {
            assert_eq!(result_path, path);
            assert_eq!(function_name, "test_function");
        } else {
            panic!("Expected Ok(TestPath::Function), got {result:?}");
        }
    }

    #[test]
    fn test_function_specification_with_auto_extension() {
        let env = TestContext::new();
        env.create_file("test.py", "def test_function(): pass");
        let base_path = env.temp_path("test");

        let function_spec = format!("{}::test_function", base_path.display());
        let result = TestPath::new(&function_spec);

        assert!(matches!(result, Ok(TestPath::Function { .. })));
        if let Ok(TestPath::Function { function_name, .. }) = result {
            assert_eq!(function_name, "test_function");
        }
    }

    #[test]
    fn test_function_specification_empty_function_name() {
        let env = TestContext::new();
        let path = env.create_file("test.py", "def test_function(): pass");

        let function_spec = format!("{}::", path.display());
        let result = TestPath::new(&function_spec);

        assert!(matches!(result, Err(TestPathError::MissingFunctionName(_))));
    }

    #[test]
    fn test_function_specification_nonexistent_file() {
        let env = TestContext::new();
        let non_existent_path = env.temp_path("nonexistent.py");

        let function_spec = format!("{}::test_function", non_existent_path.display());
        let result = TestPath::new(&function_spec);

        assert!(matches!(result, Err(TestPathError::NotFound(_))));
    }

    #[test]
    fn test_function_specification_wrong_extension() {
        let env = TestContext::new();
        let path = env.create_file("test.txt", "def test_function(): pass");

        let function_spec = format!("{}::test_function", path.display());
        let result = TestPath::new(&function_spec);

        assert!(matches!(result, Err(TestPathError::WrongFileExtension(_))));
    }

    #[test]
    fn test_try_convert_to_py_path_file() {
        let env = TestContext::new();
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
        let env = TestContext::new();
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
        let env = TestContext::new();
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
        let env = TestContext::new();
        let result = try_convert_to_py_path(&env.cwd().join("test/dir"));
        assert!(matches!(result, Err(TestPathError::NotFound(_))));
    }
}
