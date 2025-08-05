use crate::path::SystemPathBuf;

#[must_use]
pub fn is_python_file(path: &SystemPathBuf) -> bool {
    path.extension().is_some_and(|extension| extension == "py")
}

/// Gets the module name from a path.
pub fn module_name(cwd: &SystemPathBuf, path: &SystemPathBuf) -> Result<String, String> {
    let relative_path = path
        .strip_prefix(cwd)
        .map_err(|_| "Failed to get module name")?;

    let components: Vec<_> = relative_path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();

    Ok(components.join(".").trim_end_matches(".py").to_string())
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
            Ok("test".to_string())
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
            Ok("test_dir.test".to_string())
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
            Ok("tests.test".to_string())
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
                Ok("src.module.test".to_string())
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
                Ok("src.module.test".to_string())
            );
        }
    }
}
