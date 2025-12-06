use std::fmt::Formatter;

use camino::{Utf8Path, Utf8PathBuf};

use crate::utils::is_python_file;

fn try_convert_to_py_path(path: &Utf8Path) -> Result<Utf8PathBuf, TestPathError> {
    if path.exists() {
        return Ok(path.to_path_buf());
    }

    let path_with_py = Utf8PathBuf::from(format!("{path}.py"));
    if path_with_py.exists() {
        return Ok(path_with_py);
    }

    let path_with_slash = Utf8PathBuf::from(format!("{}.py", path.to_string().replace('.', "/")));
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
    File(Utf8PathBuf),

    /// A directory containing test files.
    ///
    /// Some examples are:
    /// - `tests/`
    Directory(Utf8PathBuf),

    /// A function in a file containing test functions.
    ///
    /// Some examples are:
    /// - `test_file.py::test_function`
    /// - `test_file::test_function`
    Function {
        path: Utf8PathBuf,
        function_name: String,
    },
}

impl TestPath {
    pub(crate) fn new(value: &str) -> Result<Self, TestPathError> {
        if let Some(separator_pos) = value.rfind("::") {
            let file_part = &value[..separator_pos];
            let function_name = &value[separator_pos + 2..];

            if function_name.is_empty() {
                return Err(TestPathError::MissingFunctionName(Utf8PathBuf::from(
                    file_part,
                )));
            }

            let file_path = Utf8PathBuf::from(file_part);
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
                Err(TestPathError::InvalidUtf8Path(path))
            }
        } else {
            let value = Utf8PathBuf::from(value);
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
                Err(TestPathError::InvalidUtf8Path(path))
            }
        }
    }

    pub const fn path(&self) -> &Utf8PathBuf {
        match self {
            Self::File(path) | Self::Directory(path) | Self::Function { path, .. } => path,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TestPathError {
    NotFound(Utf8PathBuf),
    WrongFileExtension(Utf8PathBuf),
    InvalidUtf8Path(Utf8PathBuf),
    MissingFunctionName(Utf8PathBuf),
}

impl TestPathError {
    pub const fn path(&self) -> &Utf8PathBuf {
        match self {
            Self::NotFound(path)
            | Self::WrongFileExtension(path)
            | Self::InvalidUtf8Path(path)
            | Self::MissingFunctionName(path) => path,
        }
    }
}

impl std::fmt::Display for TestPathError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(path) => write!(f, "path `{path}` could not be found"),
            Self::WrongFileExtension(path) => {
                write!(f, "path `{path}` has a wrong file extension")
            }
            Self::InvalidUtf8Path(path) => write!(f, "path `{path}` is invalid"),
            Self::MissingFunctionName(path) => {
                write!(f, "invalid function specification: `{path}::`")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    struct TestEnv {
        _temp_dir: tempfile::TempDir,
        base_path: Utf8PathBuf,
    }

    impl TestEnv {
        fn new() -> Self {
            let temp_dir = tempfile::tempdir().unwrap();
            let base_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
            Self {
                _temp_dir: temp_dir,
                base_path,
            }
        }

        fn create_file(&self, path: &str, content: &str) -> Utf8PathBuf {
            let file_path = self.base_path.join(path);
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&file_path, content).unwrap();
            file_path
        }

        fn create_dir(&self, path: &str) -> Utf8PathBuf {
            let dir_path = self.base_path.join(path);
            fs::create_dir_all(&dir_path).unwrap();
            dir_path
        }

        fn path(&self, path: &str) -> Utf8PathBuf {
            self.base_path.join(path)
        }
    }

    #[test]
    fn test_python_file_exact_path() {
        let env = TestEnv::new();
        let path = env.create_file("test.py", "def test(): pass");

        let result = TestPath::new(path.as_ref());

        assert!(matches!(result, Ok(TestPath::File(_))));
    }

    #[test]
    fn test_python_file_auto_extension() {
        let env = TestEnv::new();
        env.create_file("test.py", "def test(): pass");
        let path_without_ext = env.path("test");

        let result = TestPath::new(path_without_ext.as_ref());

        assert!(matches!(result, Ok(TestPath::File(_))));
    }

    #[test]
    fn test_directory_path() {
        let env = TestEnv::new();
        let dir_path = env.create_dir("test_dir");

        let result = TestPath::new(dir_path.as_ref());
        assert!(matches!(result, Ok(TestPath::Directory(_))));
    }

    #[test]
    fn test_file_not_found_exact_path() {
        let env = TestEnv::new();
        let non_existent_path = env.path("non_existent.py");

        let result = TestPath::new(non_existent_path.as_ref());
        assert!(matches!(result, Err(TestPathError::NotFound(_))));
    }

    #[test]
    fn test_file_not_found_auto_extension() {
        let env = TestEnv::new();
        let non_existent_path = env.path("non_existent");

        let result = TestPath::new(non_existent_path.as_ref());
        assert!(matches!(result, Err(TestPathError::NotFound(_))));
    }

    #[test]
    fn test_file_not_found_dotted_path() {
        let result = TestPath::new("non_existent.module");
        assert!(matches!(result, Err(TestPathError::NotFound(_))));
    }

    #[test]
    fn test_invalid_path_with_extension() {
        let env = TestEnv::new();
        let path = env.create_file("path.txt", "def test(): pass");
        let result = TestPath::new(path.as_ref());
        assert!(matches!(result, Err(TestPathError::WrongFileExtension(_))));
    }

    #[test]
    fn test_wrong_file_extension() {
        let env = TestEnv::new();
        let path = env.create_file("test.rs", "fn test() {}");

        let result = TestPath::new(path.as_ref());
        assert!(matches!(result, Err(TestPathError::WrongFileExtension(_))));
    }

    #[test]
    fn test_path_that_exists_but_is_neither_file_nor_directory() {
        let env = TestEnv::new();
        let non_existent_path = env.path("neither_file_nor_dir");

        let result = TestPath::new(non_existent_path.as_ref());
        assert!(matches!(result, Err(TestPathError::NotFound(_))));
    }

    #[test]
    fn test_file_and_auto_extension_both_exist() {
        let env = TestEnv::new();
        let test_file = env.create_file("test", "not python");
        env.create_file("test.py", "def test(): pass");

        let result = TestPath::new(test_file.as_ref());
        assert!(matches!(result, Err(TestPathError::WrongFileExtension(_))));
    }

    #[test]
    fn test_function_specification() {
        let env = TestEnv::new();
        let path = env.create_file("test.py", "def test_function(): pass");

        let function_spec = format!("{path}::test_function");
        let result = TestPath::new(&function_spec);

        if let Ok(TestPath::Function {
            path: result_path,
            function_name,
        }) = result
        {
            assert_eq!(result_path, path);
            assert_eq!(function_name, "test_function");
        } else {
            panic!("Expected Ok(TestUtf8Path::Function), got {result:?}");
        }
    }

    #[test]
    fn test_function_specification_with_auto_extension() {
        let env = TestEnv::new();
        env.create_file("test.py", "def test_function(): pass");
        let base_path = env.path("test");

        let function_spec = format!("{base_path}::test_function");
        let result = TestPath::new(&function_spec);

        assert!(matches!(result, Ok(TestPath::Function { .. })));
        if let Ok(TestPath::Function { function_name, .. }) = result {
            assert_eq!(function_name, "test_function");
        }
    }

    #[test]
    fn test_function_specification_empty_function_name() {
        let env = TestEnv::new();
        let path = env.create_file("test.py", "def test_function(): pass");

        let function_spec = format!("{path}::");
        let result = TestPath::new(&function_spec);

        assert!(matches!(result, Err(TestPathError::MissingFunctionName(_))));
    }

    #[test]
    fn test_function_specification_nonexistent_file() {
        let env = TestEnv::new();
        let non_existent_path = env.path("nonexistent.py");

        let function_spec = format!("{non_existent_path}::test_function");
        let result = TestPath::new(&function_spec);

        assert!(matches!(result, Err(TestPathError::NotFound(_))));
    }

    #[test]
    fn test_function_specification_wrong_extension() {
        let env = TestEnv::new();
        let path = env.create_file("test.txt", "def test_function(): pass");

        let function_spec = format!("{path}::test_function");
        let result = TestPath::new(&function_spec);

        assert!(matches!(result, Err(TestPathError::WrongFileExtension(_))));
    }

    #[test]
    fn test_try_convert_to_py_path_file() {
        let env = TestEnv::new();
        let py_path = env.create_file("test.py", "def test(): pass");

        let test_path = env.path("test");
        let result = try_convert_to_py_path(&test_path);
        if let Ok(path) = result {
            assert_eq!(path, py_path);
        } else {
            panic!("Expected Ok, got {result:?}");
        }
    }

    #[test]
    fn test_try_convert_to_py_path_file_slashes() {
        let env = TestEnv::new();
        let py_path = env.create_file("test/dir.py", "def test(): pass");

        let test_path = env.path("test/dir");
        let result = try_convert_to_py_path(&test_path);
        if let Ok(path) = result {
            assert_eq!(path, py_path);
        } else {
            panic!("Expected Ok, got {result:?}");
        }
    }

    #[test]
    fn test_try_convert_to_py_path_directory() {
        let env = TestEnv::new();
        let dir_path = env.create_dir("test.dir");

        let test_path = env.path("test.dir");
        let result = try_convert_to_py_path(&test_path);
        if let Ok(path) = result {
            assert_eq!(path, dir_path);
        } else {
            panic!("Expected Ok, got {result:?}");
        }
    }

    #[test]
    fn test_try_convert_to_py_path_not_found() {
        let env = TestEnv::new();
        let test_path = env.path("test/dir");
        let result = try_convert_to_py_path(&test_path);
        assert!(matches!(result, Err(TestPathError::NotFound(_))));
    }
}
