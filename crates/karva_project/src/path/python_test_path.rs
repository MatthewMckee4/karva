use std::fmt::Formatter;

use crate::{path::SystemPathBuf, utils::is_python_file};

fn try_convert_to_py_path(path: &SystemPathBuf) -> Result<SystemPathBuf, PythonTestPathError> {
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

    Err(PythonTestPathError::NotFound(path.to_string()))
}

#[derive(Eq, PartialEq, Clone, Hash, PartialOrd, Ord)]
pub enum PythonTestPath {
    File(SystemPathBuf),
    Directory(SystemPathBuf),
}

impl PythonTestPath {
    pub fn new(value: &SystemPathBuf) -> Result<Self, PythonTestPathError> {
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
            unreachable!("Path `{}` is neither a file nor a directory", path)
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

    use super::*;
    use crate::tests::TestEnv;

    #[test]
    fn test_python_file_exact_path() {
        let env = TestEnv::new();
        let path = env.create_file("test.py", "def test(): pass");

        let result = PythonTestPath::new(&path);
        assert!(matches!(result, Ok(PythonTestPath::File(_))));
    }

    #[test]
    fn test_python_file_auto_extension() {
        let env = TestEnv::new();
        env.create_file("test.py", "def test(): pass");
        let path_without_ext = env.temp_path("test");

        let result = PythonTestPath::new(&path_without_ext);
        assert!(matches!(result, Ok(PythonTestPath::File(_))));
    }

    #[test]
    fn test_directory_path() {
        let env = TestEnv::new();
        let path = env.create_dir("test_dir");

        let result = PythonTestPath::new(&path);
        assert!(matches!(result, Ok(PythonTestPath::Directory(_))));
    }

    #[test]
    fn test_file_not_found_exact_path() {
        let env = TestEnv::new();
        let non_existent_path = env.temp_path("non_existent.py");

        let result = PythonTestPath::new(&non_existent_path);
        assert!(matches!(result, Err(PythonTestPathError::NotFound(_))));
    }

    #[test]
    fn test_file_not_found_auto_extension() {
        let env = TestEnv::new();
        let non_existent_path = env.temp_path("non_existent");

        let result = PythonTestPath::new(&non_existent_path);
        assert!(matches!(result, Err(PythonTestPathError::NotFound(_))));
    }

    #[test]
    fn test_file_not_found_dotted_path() {
        let result = PythonTestPath::new(&SystemPathBuf::from("non_existent.module"));
        assert!(matches!(result, Err(PythonTestPathError::NotFound(_))));
    }

    #[test]
    fn test_wrong_file_extension() {
        let env = TestEnv::new();
        let path = env.create_file("test.rs", "fn test() {}");

        let result = PythonTestPath::new(&path);
        assert!(matches!(
            result,
            Err(PythonTestPathError::WrongFileExtension(_))
        ));
    }

    #[test]
    fn test_path_that_exists_but_is_neither_file_nor_directory() {
        let env = TestEnv::new();
        let non_existent_path = env.temp_path("neither_file_nor_dir");

        let result = PythonTestPath::new(&non_existent_path);
        assert!(matches!(result, Err(PythonTestPathError::NotFound(_))));
    }

    #[test]
    fn test_file_and_auto_extension_both_exist() {
        let env = TestEnv::new();
        env.create_file("test", "not python");
        env.create_file("test.py", "def test(): pass");
        let base_path = env.temp_path("test");

        let result = PythonTestPath::new(&base_path);
        assert!(matches!(
            result,
            Err(PythonTestPathError::WrongFileExtension(_))
        ));
    }
}
