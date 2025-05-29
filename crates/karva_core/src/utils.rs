use crate::path::SystemPathBuf;

pub fn is_python_file(path: &SystemPathBuf) -> bool {
    path.extension() == Some("py")
}

/// Gets the module name from a path.
///
/// # Panics
///
/// Panics if the path is not a valid UTF-8 path.
pub fn module_name(cwd: &SystemPathBuf, path: &SystemPathBuf) -> String {
    let relative_path = path.strip_prefix(cwd).unwrap();
    let components: Vec<_> = relative_path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    components.join(".").trim_end_matches(".py").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::SystemPathBuf;

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
