use crate::core::path::SystemPathBuf;
use std::path::MAIN_SEPARATOR;

pub fn is_python_file(path: &SystemPathBuf) -> bool {
    path.extension() == Some("py")
}

pub fn module_name(cwd: &SystemPathBuf, path: &SystemPathBuf) -> String {
    let relative_path = path.strip_prefix(cwd).unwrap();
    let path_str = relative_path.to_string();
    path_str
        .trim_end_matches(".py")
        .replace(MAIN_SEPARATOR, ".")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::path::SystemPathBuf;

    #[cfg(unix)]
    #[test]
    fn test_module_name() {
        assert_eq!(
            module_name(&SystemPathBuf::from("/"), &SystemPathBuf::from("/test.py")),
            "test"
        );
    }

    #[cfg(unix)]
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

    #[cfg(unix)]
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

    #[cfg(unix)]
    mod unix_tests {
        use super::*;

        #[test]
        fn test_unix_paths() {
            assert_eq!(
                module_name(
                    &SystemPathBuf::from("/home/user/project"),
                    &SystemPathBuf::from("/home/user/project/src/module/test.py")
                ),
                "src.module.test"
            );
        }
    }

    #[cfg(windows)]
    mod windows_tests {
        use super::*;

        #[test]
        fn test_windows_paths() {
            assert_eq!(
                module_name(
                    &SystemPathBuf::from("C:\\Users\\user\\project"),
                    &SystemPathBuf::from("C:\\Users\\user\\project\\src\\module\\test.py")
                ),
                "src.module.test"
            );
        }
    }
}
