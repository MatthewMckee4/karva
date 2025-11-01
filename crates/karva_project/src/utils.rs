use camino::{Utf8Path, Utf8PathBuf};

pub fn is_python_file(path: &Utf8Path) -> bool {
    path.extension().is_some_and(|extension| extension == "py")
}

/// Gets the module name from a path.
///
/// This can return None if the path is not relative to the current working directory.
pub fn module_name(cwd: &Utf8PathBuf, path: &Utf8Path) -> Option<String> {
    let relative_path = path.strip_prefix(cwd).ok()?;

    let components: Vec<_> = relative_path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();

    Some(components.join(".").trim_end_matches(".py").to_string())
}

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;

    use super::*;

    #[cfg(unix)]
    #[test]
    fn test_module_name() {
        assert_eq!(
            module_name(&Utf8PathBuf::from("/"), &Utf8PathBuf::from("/test.py")),
            Some("test".to_string())
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_module_name_with_directory() {
        assert_eq!(
            module_name(
                &Utf8PathBuf::from("/"),
                &Utf8PathBuf::from("/test_dir/test.py")
            ),
            Some("test_dir.test".to_string())
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_module_name_with_gitignore() {
        assert_eq!(
            module_name(
                &Utf8PathBuf::from("/"),
                &Utf8PathBuf::from("/tests/test.py")
            ),
            Some("tests.test".to_string())
        );
    }

    #[cfg(unix)]
    mod unix_tests {
        use super::*;

        #[test]
        fn test_unix_paths() {
            assert_eq!(
                module_name(
                    &Utf8PathBuf::from("/home/user/project"),
                    &Utf8PathBuf::from("/home/user/project/src/module/test.py")
                ),
                Some("src.module.test".to_string())
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
                    &Utf8PathBuf::from("C:\\Users\\user\\project"),
                    &Utf8PathBuf::from("C:\\Users\\user\\project\\src\\module\\test.py")
                ),
                Some("src.module.test".to_string())
            );
        }
    }
}
