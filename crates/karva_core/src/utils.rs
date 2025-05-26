use crate::path::SystemPathBuf;

pub fn is_python_file(path: &SystemPathBuf) -> bool {
    path.extension() == Some("py")
}

pub fn module_name(cwd: &SystemPathBuf, path: &SystemPathBuf) -> String {
    let relative_path = path.strip_prefix(cwd).unwrap();
    let path_str = relative_path.to_string();
    path_str.trim_end_matches(".py").replace('/', ".")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::SystemPathBuf;

    #[test]
    fn test_module_name() {
        assert_eq!(
            module_name(&SystemPathBuf::from("/"), &SystemPathBuf::from("/test.py")),
            "test"
        );
    }

    #[test]
    fn test_module_name_with_directory() {
        assert_eq!(
            module_name(
                &SystemPathBuf::from("/"),
                &SystemPathBuf::from("/test_dir/test.py")
            ),
            "test_dir.test"
        );
    }

    #[test]
    fn test_module_name_with_gitignore() {
        assert_eq!(
            module_name(
                &SystemPathBuf::from("/"),
                &SystemPathBuf::from("/tests/test.py")
            ),
            "tests.test"
        );
    }
}
