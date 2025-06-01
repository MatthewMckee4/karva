use std::{
    borrow::Borrow,
    fmt::Formatter,
    ops::Deref,
    path::{Path, PathBuf, StripPrefixError},
};

use camino::{Utf8Path, Utf8PathBuf};

use crate::utils::is_python_file;

#[derive(Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct SystemPath(Utf8Path);

impl SystemPath {
    pub fn new(path: &(impl AsRef<Utf8Path> + ?Sized)) -> &Self {
        let path = path.as_ref();
        unsafe { &*(std::ptr::from_ref::<Utf8Path>(path) as *const Self) }
    }

    #[inline]
    #[must_use]
    pub fn extension(&self) -> Option<&str> {
        self.0.extension()
    }

    #[inline]
    #[must_use]
    pub fn starts_with(&self, base: impl AsRef<Self>) -> bool {
        self.0.starts_with(base.as_ref())
    }

    #[inline]
    #[must_use]
    pub fn ends_with(&self, child: impl AsRef<Self>) -> bool {
        self.0.ends_with(child.as_ref())
    }

    #[inline]
    #[must_use]
    pub fn parent(&self) -> Option<&Self> {
        self.0.parent().map(Self::new)
    }

    #[inline]
    pub fn ancestors(&self) -> impl Iterator<Item = &Self> {
        self.0.ancestors().map(Self::new)
    }

    #[inline]
    pub fn components(&self) -> camino::Utf8Components {
        self.0.components()
    }

    #[inline]
    #[must_use]
    pub fn file_name(&self) -> Option<&str> {
        self.0.file_name()
    }

    #[inline]
    #[must_use]
    pub fn file_stem(&self) -> Option<&str> {
        self.0.file_stem()
    }

    /// Strips the prefix from the path.
    ///
    /// # Errors
    ///
    /// This function will return an error if the path is not a valid UTF-8 path.
    #[inline]
    pub fn strip_prefix(
        &self,
        base: impl AsRef<Self>,
    ) -> std::result::Result<&Self, StripPrefixError> {
        self.0.strip_prefix(base.as_ref()).map(Self::new)
    }

    #[inline]
    #[must_use]
    pub fn join(&self, path: impl AsRef<Self>) -> SystemPathBuf {
        SystemPathBuf::from_utf8_path_buf(self.0.join(&path.as_ref().0))
    }

    #[inline]
    #[must_use]
    pub fn with_extension(&self, extension: &str) -> SystemPathBuf {
        SystemPathBuf::from_utf8_path_buf(self.0.with_extension(extension))
    }

    #[must_use]
    pub fn to_path_buf(&self) -> SystemPathBuf {
        SystemPathBuf(self.0.to_path_buf())
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    #[inline]
    #[must_use]
    pub fn as_std_path(&self) -> &Path {
        self.0.as_std_path()
    }

    #[inline]
    #[must_use]
    pub const fn as_utf8_path(&self) -> &Utf8Path {
        &self.0
    }

    #[must_use]
    pub fn from_std_path(path: &Path) -> Option<&Self> {
        Some(Self::new(Utf8Path::from_path(path)?))
    }

    pub fn absolute(path: impl AsRef<Self>, cwd: impl AsRef<Self>) -> SystemPathBuf {
        fn absolute(path: &SystemPath, cwd: &SystemPath) -> SystemPathBuf {
            let path = &path.0;

            let mut components = path.components().peekable();
            let mut ret = if let Some(
                c @ (camino::Utf8Component::Prefix(..) | camino::Utf8Component::RootDir),
            ) = components.peek().copied()
            {
                components.next();
                Utf8PathBuf::from(c.as_str())
            } else {
                cwd.0.to_path_buf()
            };

            for component in components {
                match component {
                    camino::Utf8Component::Prefix(..) => unreachable!(),
                    camino::Utf8Component::RootDir => {
                        ret.push(component);
                    }
                    camino::Utf8Component::CurDir => {}
                    camino::Utf8Component::ParentDir => {
                        ret.pop();
                    }
                    camino::Utf8Component::Normal(c) => {
                        ret.push(c);
                    }
                }
            }

            SystemPathBuf::from_utf8_path_buf(ret)
        }

        absolute(path.as_ref(), cwd.as_ref())
    }

    #[must_use]
    pub fn is_file(&self) -> bool {
        self.0.is_file()
    }

    #[must_use]
    pub fn is_dir(&self) -> bool {
        self.0.is_dir()
    }
}

impl ToOwned for SystemPath {
    type Owned = SystemPathBuf;

    fn to_owned(&self) -> Self::Owned {
        self.to_path_buf()
    }
}

#[derive(Eq, PartialEq, Clone, Hash, PartialOrd, Ord)]
pub struct SystemPathBuf(Utf8PathBuf);

impl SystemPathBuf {
    #[must_use]
    pub fn new() -> Self {
        Self(Utf8PathBuf::new())
    }

    #[must_use]
    pub const fn from_utf8_path_buf(path: Utf8PathBuf) -> Self {
        Self(path)
    }

    /// Creates a new [`SystemPathBuf`] from a [`PathBuf`].
    ///
    /// # Errors
    ///
    /// This function will return an error if the path is not a valid UTF-8 path.
    pub fn from_path_buf(
        path: std::path::PathBuf,
    ) -> std::result::Result<Self, std::path::PathBuf> {
        Utf8PathBuf::from_path_buf(path).map(Self)
    }

    pub fn push(&mut self, path: impl AsRef<SystemPath>) {
        self.0.push(&path.as_ref().0);
    }

    #[must_use]
    pub fn into_utf8_path_buf(self) -> Utf8PathBuf {
        self.0
    }

    #[must_use]
    pub fn into_std_path_buf(self) -> PathBuf {
        self.0.into_std_path_buf()
    }

    #[inline]
    #[must_use]
    pub fn as_path(&self) -> &SystemPath {
        SystemPath::new(&self.0)
    }

    #[must_use]
    pub fn is_file(&self) -> bool {
        self.0.is_file()
    }

    #[must_use]
    pub fn is_dir(&self) -> bool {
        self.0.is_dir()
    }

    #[must_use]
    pub fn exists(&self) -> bool {
        self.0.exists()
    }
}

impl Borrow<SystemPath> for SystemPathBuf {
    fn borrow(&self) -> &SystemPath {
        self.as_path()
    }
}

impl From<&str> for SystemPathBuf {
    fn from(value: &str) -> Self {
        Self::from_utf8_path_buf(Utf8PathBuf::from(value))
    }
}

impl From<String> for SystemPathBuf {
    fn from(value: String) -> Self {
        Self::from_utf8_path_buf(Utf8PathBuf::from(value))
    }
}

impl Default for SystemPathBuf {
    fn default() -> Self {
        Self::new()
    }
}

impl AsRef<SystemPath> for SystemPathBuf {
    #[inline]
    fn as_ref(&self) -> &SystemPath {
        self.as_path()
    }
}

impl AsRef<Self> for SystemPath {
    #[inline]
    fn as_ref(&self) -> &Self {
        self
    }
}

impl AsRef<SystemPath> for Utf8Path {
    #[inline]
    fn as_ref(&self) -> &SystemPath {
        SystemPath::new(self)
    }
}

impl AsRef<SystemPath> for Utf8PathBuf {
    #[inline]
    fn as_ref(&self) -> &SystemPath {
        SystemPath::new(self.as_path())
    }
}

impl AsRef<SystemPath> for str {
    #[inline]
    fn as_ref(&self) -> &SystemPath {
        SystemPath::new(self)
    }
}

impl AsRef<SystemPath> for String {
    #[inline]
    fn as_ref(&self) -> &SystemPath {
        SystemPath::new(self)
    }
}

impl AsRef<Path> for SystemPath {
    #[inline]
    fn as_ref(&self) -> &Path {
        self.0.as_std_path()
    }
}

impl Deref for SystemPathBuf {
    type Target = SystemPath;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_path()
    }
}

impl std::fmt::Debug for SystemPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::fmt::Display for SystemPath {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::fmt::Debug for SystemPathBuf {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::fmt::Display for SystemPathBuf {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<&Path> for SystemPathBuf {
    fn from(value: &Path) -> Self {
        Self::from_utf8_path_buf(
            Utf8PathBuf::from_path_buf(value.to_path_buf()).unwrap_or_default(),
        )
    }
}

impl From<PathBuf> for SystemPathBuf {
    fn from(value: PathBuf) -> Self {
        Self::from_utf8_path_buf(Utf8PathBuf::from_path_buf(value).unwrap_or_default())
    }
}

pub fn deduplicate_nested_paths<P, I>(paths: I) -> DeduplicatedNestedPathsIter<P>
where
    I: IntoIterator<Item = P>,
    P: AsRef<str>,
{
    DeduplicatedNestedPathsIter::new(paths)
}

pub struct DeduplicatedNestedPathsIter<P> {
    inner: std::vec::IntoIter<P>,
    next: Option<P>,
}

impl<P> DeduplicatedNestedPathsIter<P>
where
    P: AsRef<str>,
{
    fn new<I>(paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
    {
        let mut paths = paths.into_iter().collect::<Vec<_>>();

        // Sort the path to ensure that e.g. `/a/b/c`, comes right after `/a/b`.
        paths.sort_unstable_by(|left, right| left.as_ref().cmp(right.as_ref()));

        let mut iter = paths.into_iter();

        Self {
            next: iter.next(),
            inner: iter,
        }
    }
}

impl<P> Iterator for DeduplicatedNestedPathsIter<P>
where
    P: AsRef<str>,
{
    type Item = P;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.next.take()?;

        for next in self.inner.by_ref() {
            // Skip all paths that have the same prefix as the current path
            if !next.as_ref().starts_with(current.as_ref()) {
                self.next = Some(next);
                break;
            }
        }

        Some(current)
    }
}

fn try_convert_to_py_path(path: &str) -> Result<SystemPathBuf, PythonTestPathError> {
    let file_path = SystemPathBuf::from(path);
    if file_path.exists() {
        return Ok(file_path);
    }

    let path_with_py = SystemPathBuf::from(format!("{path}.py"));
    if path_with_py.exists() {
        return Ok(path_with_py);
    }

    let path_with_slash = SystemPathBuf::from(format!("{}.py", path.replace('.', "/")));
    if path_with_slash.exists() {
        return Ok(path_with_slash);
    }

    Err(PythonTestPathError::NotFound(file_path.to_string()))
}

#[derive(Eq, PartialEq, Clone, Hash, PartialOrd, Ord)]
pub enum PythonTestPath {
    File(SystemPathBuf),
    Directory(SystemPathBuf),
    Function(SystemPathBuf, String),
}

impl PythonTestPath {
    /// Creates a new [`PythonTestPath`] from a [`SystemPathBuf`].
    ///
    /// # Errors
    ///
    /// This function will return an error if the path is not a valid Python test path.
    pub fn new(value: impl AsRef<str>) -> Result<Self, PythonTestPathError> {
        let value = value.as_ref();
        if value.contains("::") {
            let parts: Vec<String> = value.split("::").map(ToString::to_string).collect();
            match parts.as_slice() {
                [file, function] => {
                    let file_path = try_convert_to_py_path(file.as_str())?;

                    if file_path.is_file() {
                        if is_python_file(&file_path) {
                            Ok(Self::Function(file_path, function.to_string()))
                        } else {
                            Err(PythonTestPathError::WrongFileExtension(file.clone()))
                        }
                    } else {
                        Err(PythonTestPathError::InvalidPath(file.clone()))
                    }
                }
                _ => Err(PythonTestPathError::InvalidPath(value.to_string())),
            }
        } else {
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
                Err(PythonTestPathError::InvalidPath(path.to_string()))
            }
        }
    }
}

impl std::fmt::Debug for PythonTestPath {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(path) => write!(f, "File: {path}"),
            Self::Directory(path) => write!(f, "Directory: {path}"),
            Self::Function(path, function) => write!(f, "Function: {path}::{function}"),
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
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    struct TestEnv {
        temp_dir: TempDir,
    }

    impl TestEnv {
        fn new() -> Self {
            Self {
                temp_dir: TempDir::new().expect("Failed to create temp directory"),
            }
        }

        fn create_test_file(&self, name: &str, content: &str) -> std::io::Result<String> {
            let path = self.temp_dir.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, content)?;
            Ok(path.display().to_string())
        }

        fn create_test_dir(&self, name: &str) -> std::io::Result<String> {
            let path = self.temp_dir.path().join(name);
            fs::create_dir_all(&path)?;
            Ok(path.display().to_string())
        }
    }

    #[test]
    fn test_file_path_creation() -> std::io::Result<()> {
        let env = TestEnv::new();
        let path = env.create_test_file("test_file.py", "def test_function(): assert(True)")?;

        let test_path = PythonTestPath::new(path).expect("Failed to create file path");

        match test_path {
            PythonTestPath::File(file) => {
                assert!(file.as_str().ends_with("test_file.py"));
            }
            _ => panic!("Expected File variant"),
        }

        Ok(())
    }

    #[test]
    fn test_directory_path_creation() -> std::io::Result<()> {
        let env = TestEnv::new();
        let path = env.create_test_dir("test_dir")?;

        let test_path = PythonTestPath::new(&path).expect("Failed to create directory path");

        match test_path {
            PythonTestPath::Directory(dir) => {
                assert!(dir.as_str().ends_with("test_dir"));
            }
            _ => panic!("Expected Directory variant"),
        }

        Ok(())
    }

    #[test]
    fn test_function_path_creation_py_extension() -> std::io::Result<()> {
        let env = TestEnv::new();
        let file_path =
            env.create_test_file("function_test.py", "def test_function(): assert True")?;

        let test_path = PythonTestPath::new(format!("{file_path}::test_function"));

        match test_path {
            Ok(PythonTestPath::Function(file, func)) => {
                assert!(file.as_str().ends_with("function_test.py"));
                assert_eq!(func, "test_function");
            }
            _ => panic!("Expected Function variant"),
        }

        Ok(())
    }

    #[test]
    fn test_function_path_creation_no_extension() -> std::io::Result<()> {
        let env = TestEnv::new();

        env.create_test_file("function_test.py", "def test_function(): assert True")?;

        let path_without_py = env.temp_dir.path().join("function_test");

        let func_path = format!("{}::test_function", path_without_py.display());
        let test_path = PythonTestPath::new(&func_path);

        match test_path {
            Ok(PythonTestPath::Function(file, func)) => {
                assert!(file.as_str().ends_with("function_test.py"));
                assert_eq!(func, "test_function");
            }
            _ => panic!("Expected Function variant"),
        }

        Ok(())
    }

    #[test]
    fn test_invalid_paths() {
        let env = TestEnv::new();
        let non_existent_path = env.temp_dir.path().join("non_existent.py");

        assert!(!non_existent_path.exists());

        let res = PythonTestPath::new(non_existent_path.display().to_string());

        assert!(matches!(res, Err(PythonTestPathError::NotFound(_))));

        assert!(matches!(
            PythonTestPath::new(format!("{}::function", non_existent_path.display())),
            Err(PythonTestPathError::NotFound(_))
        ));
    }

    #[test]
    fn test_wrong_file_extension() -> std::io::Result<()> {
        let env = TestEnv::new();
        let path = env.create_test_file("wrong_ext.rs", "fn test_function() { assert!(true); }")?;

        assert!(matches!(
            PythonTestPath::new(&path),
            Err(PythonTestPathError::WrongFileExtension(_))
        ));

        assert!(matches!(
            PythonTestPath::new(format!("{}::test_function", path.as_str())),
            Err(PythonTestPathError::WrongFileExtension(_))
        ));

        Ok(())
    }

    #[test]
    fn test_deduplicate_nested_paths() {
        let dirs = [
            "/a",
            "/a/bar",
            "/b/bar",
            "/b/bar::test_function",
            "/c/bar",
            "/c/bar::test_function",
            "/d/bar",
            "/d/bar::test_function",
            "/e/bar::test_function",
            "/e/bar",
        ];

        let deduped_dirs = deduplicate_nested_paths(dirs);

        assert_eq!(
            deduped_dirs.into_iter().collect::<Vec<_>>(),
            vec!["/a", "/b/bar", "/c/bar", "/d/bar", "/e/bar"]
        );
    }
}
