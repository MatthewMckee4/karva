use ruff_python_ast::PythonVersion;

use crate::path::{PythonTestPath, PythonTestPathError, SystemPath, SystemPathBuf};

#[derive(Default, Clone)]
pub struct ProjectMetadata {
    pub python_version: PythonVersion,
}

#[derive(Clone)]
pub struct ProjectOptions {
    pub test_prefix: String,
}

impl Default for ProjectOptions {
    fn default() -> Self {
        Self {
            test_prefix: "test".to_string(),
        }
    }
}

#[derive(Clone)]
pub struct Project {
    cwd: SystemPathBuf,
    paths: Vec<SystemPathBuf>,
    metadata: ProjectMetadata,
    options: ProjectOptions,
}

impl Project {
    #[must_use]
    pub fn new(cwd: SystemPathBuf, paths: Vec<SystemPathBuf>) -> Self {
        Self {
            cwd,
            paths,
            metadata: ProjectMetadata::default(),
            options: ProjectOptions::default(),
        }
    }

    #[must_use]
    pub const fn with_metadata(mut self, config: ProjectMetadata) -> Self {
        self.metadata = config;
        self
    }

    #[must_use]
    pub fn with_options(mut self, options: ProjectOptions) -> Self {
        self.options = options;
        self
    }

    #[must_use]
    pub const fn cwd(&self) -> &SystemPathBuf {
        &self.cwd
    }

    #[must_use]
    pub fn paths(&self) -> &[SystemPathBuf] {
        &self.paths
    }

    #[must_use]
    pub fn python_test_paths(&self) -> Vec<Result<PythonTestPath, PythonTestPathError>> {
        self.paths.iter().map(PythonTestPath::new).collect()
    }

    /// Get the common parent directory of the test paths.
    ///
    /// This is used to determine the highest common directory containing all test paths.
    #[must_use]
    pub fn parent_test_path(&self) -> SystemPathBuf {
        if self.paths.is_empty() {
            self.cwd.clone()
        } else {
            parent_of_all(&self.paths)
        }
    }

    #[must_use]
    pub fn test_prefix(&self) -> &str {
        &self.options.test_prefix
    }

    #[must_use]
    pub const fn python_version(&self) -> &PythonVersion {
        &self.metadata.python_version
    }
}

/// Get the common parent directory of the test paths.
///
/// This is used to determine the highest common directory containing all test paths.
#[must_use]
fn parent_of_all(paths: &[SystemPathBuf]) -> SystemPathBuf {
    if paths.is_empty() {
        return SystemPathBuf::new();
    }

    // Normalize paths: if a path is a file, get its parent directory
    let normalized_paths: Vec<SystemPathBuf> = paths
        .iter()
        .map(|path| {
            if path.is_file() {
                path.parent()
                    .map_or_else(|| path.clone(), SystemPath::to_path_buf)
            } else {
                path.clone()
            }
        })
        .collect();

    let mut common_prefix = normalized_paths[0].clone();

    for path in &normalized_paths[1..] {
        let mut new_prefix = SystemPathBuf::new();
        let mut common_components = common_prefix.components();
        let mut path_components = path.components();

        while let (Some(common_component), Some(path_component)) =
            (common_components.next(), path_components.next())
        {
            if common_component == path_component {
                new_prefix = new_prefix.join(common_component.as_str());
            } else {
                break;
            }
        }
        common_prefix = new_prefix;
    }

    common_prefix
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::TestEnv;

    #[test]
    fn test_parent_of_all() {
        let test_env = TestEnv::new();
        let path1 = test_env.create_dir("a/b/c");
        let path2 = test_env.create_dir("a/b/d");

        let paths = vec![path1, path2];
        let parent = parent_of_all(&paths);
        let expected = test_env.cwd().join("a/b");

        assert_eq!(parent, expected);
    }

    #[test]
    fn test_parent_of_all_with_empty_paths() {
        let paths = vec![];
        let parent = parent_of_all(&paths);
        assert_eq!(parent, SystemPathBuf::new());
    }

    #[test]
    fn test_parent_of_all_with_one_path() {
        let test_env = TestEnv::new();
        let path1 = test_env.create_dir("a/b/c");

        let paths = vec![path1.clone()];
        let parent = parent_of_all(&paths);

        assert_eq!(parent, path1);
    }

    #[test]
    fn test_parent_of_all_with_multiple_paths() {
        let test_env = TestEnv::new();
        let path1 = test_env.create_dir("a/b/c");
        let path2 = test_env.create_dir("a/b/d");

        let paths = vec![path1, path2];
        let parent = parent_of_all(&paths);
        let expected = test_env.cwd().join("a/b");

        assert_eq!(parent, expected);
    }

    #[test]
    fn test_parent_of_all_with_multiple_paths_and_different_depths() {
        let test_env = TestEnv::new();
        let path1 = test_env.create_dir("a/b/c");
        let path2 = test_env.create_dir("a/b/d/e");

        let paths = vec![path1, path2];
        let parent = parent_of_all(&paths);
        let expected = test_env.cwd().join("a/b");

        assert_eq!(parent, expected);
    }

    #[test]
    fn test_parent_of_all_with_files() {
        let test_env = TestEnv::new();
        let path1 = test_env.create_file("a/b/c/test.py", "def test(): pass");
        let path2 = test_env.create_file("a/b/d/test.py", "def test(): pass");

        let paths = vec![path1, path2];
        let parent = parent_of_all(&paths);
        let expected = test_env.cwd().join("a/b");

        assert_eq!(parent, expected);
    }

    #[test]
    fn test_parent_of_all_with_mixed_files_and_dirs() {
        let test_env = TestEnv::new();
        let path1 = test_env.create_file("a/b/c/test.py", "def test(): pass");
        let path2 = test_env.create_dir("a/b/d");

        let paths = vec![path1, path2];
        let parent = parent_of_all(&paths);
        let expected = test_env.cwd().join("a/b");

        assert_eq!(parent, expected);
    }

    #[test]
    fn test_parent_of_single_file() {
        let test_env = TestEnv::new();
        let file_path = test_env.create_file("a/b/c/test.py", "def test(): pass");

        let paths = vec![file_path];
        let parent = parent_of_all(&paths);
        let expected = test_env.cwd().join("a/b/c");

        assert_eq!(parent, expected);
    }
}
