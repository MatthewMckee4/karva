use std::{
    borrow::Borrow,
    fmt::Formatter,
    ops::Deref,
    path::{Path, PathBuf, StripPrefixError},
};

use camino::{Utf8Path, Utf8PathBuf};

pub mod python_test_path;

pub use python_test_path::{PythonTestPath, PythonTestPathError};

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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use camino::{Utf8Path, Utf8PathBuf};

    use super::*;

    fn normalize_path_for_test(path: &str) -> String {
        path.replace('\\', "/")
    }

    #[test]
    fn test_system_path_new() {
        let path_str = "/home/user/file.txt";
        let utf8_path = Utf8Path::new(path_str);
        let system_path = SystemPath::new(utf8_path);
        assert_eq!(system_path.as_str(), path_str);
    }

    #[test]
    fn test_system_path_extension() {
        let path = SystemPath::new("file.txt");
        assert_eq!(path.extension(), Some("txt"));

        let path_no_ext = SystemPath::new("file");
        assert_eq!(path_no_ext.extension(), None);

        let path_hidden = SystemPath::new(".hidden");
        assert_eq!(path_hidden.extension(), None);
    }

    #[test]
    fn test_system_path_starts_with() {
        let path = SystemPath::new("home/user/documents/file.txt");
        let base = SystemPath::new("home/user");
        assert!(path.starts_with(base));

        let wrong_base = SystemPath::new("home/other");
        assert!(!path.starts_with(wrong_base));
    }

    #[test]
    fn test_system_path_ends_with() {
        let path = SystemPath::new("home/user/documents/file.txt");
        let ending = SystemPath::new("documents/file.txt");
        assert!(path.ends_with(ending));

        let wrong_ending = SystemPath::new("other/file.txt");
        assert!(!path.ends_with(wrong_ending));
    }

    #[test]
    fn test_system_path_parent() {
        let path = SystemPath::new("home/user/file.txt");
        let parent = path.parent().unwrap();
        assert_eq!(normalize_path_for_test(parent.as_str()), "home/user");

        let root = SystemPath::new("/");
        assert!(root.parent().is_none());
    }

    #[test]
    fn test_system_path_ancestors() {
        let path = SystemPath::new("home/user/documents");
        let ancestors: Vec<String> = path
            .ancestors()
            .map(|p| normalize_path_for_test(p.as_str()))
            .collect();
        assert_eq!(
            ancestors,
            vec!["home/user/documents", "home/user", "home", ""]
        );
    }

    #[test]
    fn test_system_path_components() {
        let path = SystemPath::new("/home/user/file.txt");
        let components: Vec<_> = {
            let this = path.components();
            this.collect::<Vec<_>>()
        };
        assert!(components.len() >= 3);
    }

    #[test]
    fn test_system_path_file_name() {
        let path = SystemPath::new("home/user/file.txt");
        assert_eq!(path.file_name(), Some("file.txt"));

        let dir_path = SystemPath::new("home/user/");
        assert_eq!(dir_path.file_name(), Some("user"));

        let root = SystemPath::new("/");
        assert_eq!(root.file_name(), None);
    }

    #[test]
    fn test_system_path_file_stem() {
        let path = SystemPath::new("file.txt");
        assert_eq!(path.file_stem(), Some("file"));

        let path_no_ext = SystemPath::new("file");
        assert_eq!(path_no_ext.file_stem(), Some("file"));

        let path_multiple_ext = SystemPath::new("file.tar.gz");
        assert_eq!(path_multiple_ext.file_stem(), Some("file.tar"));
    }

    #[test]
    fn test_system_path_strip_prefix() {
        let path = SystemPath::new("home/user/documents/file.txt");
        let base = SystemPath::new("home/user");
        let stripped = path.strip_prefix(base).unwrap();
        assert_eq!(
            normalize_path_for_test(stripped.as_str()),
            "documents/file.txt"
        );

        let wrong_base = SystemPath::new("other");
        assert!(path.strip_prefix(wrong_base).is_err());
    }

    #[test]
    fn test_system_path_join() {
        let base = SystemPath::new("home/user");
        let relative = SystemPath::new("documents/file.txt");
        let joined = base.join(relative);
        assert_eq!(
            normalize_path_for_test(joined.as_str()),
            "home/user/documents/file.txt"
        );
    }

    #[test]
    fn test_system_path_with_extension() {
        let path = SystemPath::new("file.txt");
        let new_path = path.with_extension("md");
        assert_eq!(new_path.as_str(), "file.md");

        let path_no_ext = SystemPath::new("file");
        let with_ext = path_no_ext.with_extension("txt");
        assert_eq!(with_ext.as_str(), "file.txt");
    }

    #[test]
    fn test_system_path_to_path_buf() {
        let path = SystemPath::new("home/user/file.txt");
        let path_buf = path.to_path_buf();
        assert_eq!(
            normalize_path_for_test(path_buf.as_str()),
            "home/user/file.txt"
        );
    }

    #[test]
    fn test_system_path_as_str() {
        let path = SystemPath::new("home/user/file.txt");
        assert_eq!(normalize_path_for_test(path.as_str()), "home/user/file.txt");
    }

    #[test]
    fn test_system_path_as_std_path() {
        let path = SystemPath::new("home/user/file.txt");
        let std_path = path.as_std_path();
        assert_eq!(
            normalize_path_for_test(&std_path.to_string_lossy()),
            "home/user/file.txt"
        );
    }

    #[test]
    fn test_system_path_as_utf8_path() {
        let original = Utf8Path::new("home/user/file.txt");
        let system_path = SystemPath::new(original);
        let utf8_path = system_path.as_utf8_path();
        assert_eq!(utf8_path, original);
    }

    #[test]
    fn test_system_path_from_std_path() {
        let std_path = Path::new("home/user/file.txt");
        let system_path = SystemPath::from_std_path(std_path).unwrap();
        assert_eq!(
            normalize_path_for_test(system_path.as_str()),
            "home/user/file.txt"
        );
    }

    #[test]
    fn test_system_path_absolute() {
        let relative = SystemPath::new("documents/file.txt");
        let cwd = SystemPath::new("C:/home/user");
        let absolute = SystemPath::absolute(relative, cwd);
        let normalized_result = normalize_path_for_test(absolute.as_str());
        assert!(normalized_result.ends_with("/home/user/documents/file.txt"));

        let already_absolute = if cfg!(windows) {
            SystemPath::new("C:/tmp/file.txt")
        } else {
            SystemPath::new("/tmp/file.txt")
        };
        let absolute2 = SystemPath::absolute(already_absolute, cwd);
        let normalized_result2 = normalize_path_for_test(absolute2.as_str());
        assert!(normalized_result2.contains("tmp/file.txt"));

        let with_parent = SystemPath::new("../other/file.txt");
        let absolute3 = SystemPath::absolute(with_parent, SystemPath::new("C:/home/user/current"));
        let normalized_result3 = normalize_path_for_test(absolute3.as_str());
        assert!(normalized_result3.ends_with("/home/user/other/file.txt"));

        let with_current = SystemPath::new("./file.txt");
        let absolute4 = SystemPath::absolute(with_current, cwd);
        let normalized_result4 = normalize_path_for_test(absolute4.as_str());
        assert!(normalized_result4.ends_with("/home/user/file.txt"));
    }

    #[test]
    fn test_system_path_buf_new() {
        let path_buf = SystemPathBuf::new();
        assert_eq!(path_buf.as_str(), "");
    }

    #[test]
    fn test_system_path_buf_from_utf8_path_buf() {
        let utf8_path_buf = Utf8PathBuf::from("home/user/file.txt");
        let system_path_buf = SystemPathBuf::from_utf8_path_buf(utf8_path_buf);
        assert_eq!(
            normalize_path_for_test(system_path_buf.as_str()),
            "home/user/file.txt"
        );
    }

    #[test]
    fn test_system_path_buf_from_path_buf() {
        let std_path_buf = std::path::PathBuf::from("home/user/file.txt");
        let system_path_buf = SystemPathBuf::from_path_buf(std_path_buf).unwrap();
        assert_eq!(
            normalize_path_for_test(system_path_buf.as_str()),
            "home/user/file.txt"
        );
    }

    #[test]
    fn test_system_path_buf_push() {
        let mut path_buf = SystemPathBuf::from("home/user");
        path_buf.push(SystemPath::new("documents"));
        path_buf.push(SystemPath::new("file.txt"));
        assert_eq!(
            normalize_path_for_test(path_buf.as_str()),
            "home/user/documents/file.txt"
        );
    }

    #[test]
    fn test_system_path_buf_into_utf8_path_buf() {
        let system_path_buf = SystemPathBuf::from("home/user/file.txt");
        let utf8_path_buf = system_path_buf.into_utf8_path_buf();
        assert_eq!(
            normalize_path_for_test(utf8_path_buf.as_str()),
            "home/user/file.txt"
        );
    }

    #[test]
    fn test_system_path_buf_into_std_path_buf() {
        let system_path_buf = SystemPathBuf::from("home/user/file.txt");
        let std_path_buf = system_path_buf.into_std_path_buf();
        let expected = std::path::PathBuf::from("home/user/file.txt");
        assert_eq!(
            normalize_path_for_test(&std_path_buf.to_string_lossy()),
            normalize_path_for_test(&expected.to_string_lossy())
        );
    }

    #[test]
    fn test_system_path_buf_as_path() {
        let path_buf = SystemPathBuf::from("home/user/file.txt");
        let path = path_buf.as_path();
        assert_eq!(normalize_path_for_test(path.as_str()), "home/user/file.txt");
    }

    #[test]
    fn test_system_path_buf_deref() {
        let path_buf = SystemPathBuf::from("home/user/file.txt");
        assert_eq!(path_buf.file_name(), Some("file.txt"));
        assert_eq!(path_buf.extension(), Some("txt"));
    }

    #[test]
    fn test_system_path_buf_borrow() {
        use std::borrow::Borrow;

        let path_buf = SystemPathBuf::from("home/user/file.txt");
        let path: &SystemPath = path_buf.borrow();
        assert_eq!(normalize_path_for_test(path.as_str()), "home/user/file.txt");
    }

    #[test]
    fn test_system_path_to_owned() {
        let path = SystemPath::new("home/user/file.txt");
        let owned = path.to_owned();
        assert_eq!(
            normalize_path_for_test(owned.as_str()),
            "home/user/file.txt"
        );
    }

    #[test]
    fn test_from_implementations() {
        let from_str = SystemPathBuf::from("home/user/file.txt");
        assert_eq!(
            normalize_path_for_test(from_str.as_str()),
            "home/user/file.txt"
        );

        let from_string = SystemPathBuf::from(String::from("home/user/file.txt"));
        assert_eq!(
            normalize_path_for_test(from_string.as_str()),
            "home/user/file.txt"
        );

        let std_path = Path::new("home/user/file.txt");
        let from_path = SystemPathBuf::from(std_path);
        assert_eq!(
            normalize_path_for_test(from_path.as_str()),
            "home/user/file.txt"
        );

        let std_path_buf = std::path::PathBuf::from("home/user/file.txt");
        let from_path_buf = SystemPathBuf::from(std_path_buf);
        assert_eq!(
            normalize_path_for_test(from_path_buf.as_str()),
            "home/user/file.txt"
        );
    }

    #[test]
    fn test_as_ref_implementations() {
        let path_buf = SystemPathBuf::from("home/user/file.txt");
        let utf8_path = Utf8Path::new("home/user/file.txt");
        let utf8_path_buf = Utf8PathBuf::from("home/user/file.txt");
        let str_ref = "home/user/file.txt";
        let string = String::from("home/user/file.txt");

        let _: &SystemPath = path_buf.as_ref();
        let _: &SystemPath = utf8_path.as_ref();
        let _: &SystemPath = utf8_path_buf.as_ref();
        let _: &SystemPath = str_ref.as_ref();
        let _: &SystemPath = string.as_ref();

        let system_path = SystemPath::new("home/user/file.txt");
        let std_path_ref: &Path = system_path.as_ref();
        let expected = Path::new("home/user/file.txt");
        assert_eq!(
            normalize_path_for_test(&std_path_ref.to_string_lossy()),
            normalize_path_for_test(&expected.to_string_lossy())
        );
    }

    #[test]
    fn test_default_implementation() {
        let default_path = SystemPathBuf::default();
        assert_eq!(default_path.as_str(), "");
        assert_eq!(default_path, SystemPathBuf::new());
    }

    #[test]
    fn test_debug_and_display() {
        let path = SystemPath::new("home/user/file.txt");
        let path_buf = SystemPathBuf::from("home/user/file.txt");

        let debug_path = format!("{path:?}");
        let debug_path_buf = format!("{path_buf:?}");
        assert!(
            debug_path.contains("home")
                && debug_path.contains("user")
                && debug_path.contains("file.txt")
        );
        assert!(
            debug_path_buf.contains("home")
                && debug_path_buf.contains("user")
                && debug_path_buf.contains("file.txt")
        );

        let display_path = normalize_path_for_test(&format!("{path}"));
        let display_path_buf = normalize_path_for_test(&format!("{path_buf}"));
        assert_eq!(display_path, "home/user/file.txt");
        assert_eq!(display_path_buf, "home/user/file.txt");
    }

    #[test]
    fn test_equality_and_ordering() {
        let path1 = SystemPath::new("home/user/a.txt");
        let path2 = SystemPath::new("home/user/b.txt");
        let path1_clone = SystemPath::new("home/user/a.txt");

        assert_eq!(path1, path1_clone);
        assert_ne!(path1, path2);

        assert!(path1 < path2);
        assert!(path2 > path1);

        let path_buf1 = SystemPathBuf::from("home/user/a.txt");
        let path_buf2 = SystemPathBuf::from("home/user/b.txt");
        let path_buf1_clone = SystemPathBuf::from("home/user/a.txt");

        assert_eq!(path_buf1, path_buf1_clone);
        assert_ne!(path_buf1, path_buf2);

        assert!(path_buf1 < path_buf2);
        assert!(path_buf2 > path_buf1);
    }

    #[test]
    fn test_hash() {
        use std::collections::HashMap;

        let mut map = HashMap::new();
        let path = SystemPath::new("home/user/file.txt");
        let path_buf = SystemPathBuf::from("home/user/file.txt");

        map.insert(path.to_path_buf(), "value1");
        map.insert(path_buf, "value2");

        assert_eq!(map.len(), 1);
    }

    #[test]
    fn test_edge_cases() {
        let empty = SystemPath::new("");
        assert_eq!(empty.as_str(), "");
        assert_eq!(empty.file_name(), None);
        assert_eq!(empty.parent(), None);

        let root = SystemPath::new("/");
        assert_eq!(root.as_str(), "/");
        assert_eq!(root.file_name(), None);
        assert_eq!(root.parent(), None);

        let file_only = SystemPath::new("file.txt");
        assert_eq!(file_only.file_name(), Some("file.txt"));
        assert_eq!(file_only.parent(), Some(SystemPath::new("")));

        let with_dots = SystemPath::new("./file.txt");
        assert!(with_dots.as_str().contains("file.txt"));

        let with_double_dots = SystemPath::new("../file.txt");
        assert!(with_double_dots.as_str().contains("file.txt"));
    }

    #[test]
    fn test_absolute_edge_cases() {
        let cwd = SystemPath::new("C:/home/user");

        let many_parents = SystemPath::new("../../other/file.txt");
        let absolute = SystemPath::absolute(many_parents, cwd);
        let normalized = normalize_path_for_test(absolute.as_str());
        assert!(normalized.contains("other/file.txt"));

        let current_dir = SystemPath::new(".");
        let absolute_current = SystemPath::absolute(current_dir, cwd);
        let normalized_current = normalize_path_for_test(absolute_current.as_str());
        assert!(
            normalized_current.ends_with("/home/user")
                || normalized_current.ends_with("\\home\\user")
        );

        let parent_dir = SystemPath::new("..");
        let absolute_parent = SystemPath::absolute(parent_dir, cwd);
        let normalized_parent = normalize_path_for_test(absolute_parent.as_str());
        assert!(normalized_parent.contains("home"));

        let complex = SystemPath::new("./docs/../src/./main.rs");
        let absolute_complex = SystemPath::absolute(complex, cwd);
        let normalized_complex = normalize_path_for_test(absolute_complex.as_str());
        assert!(normalized_complex.contains("src") && normalized_complex.contains("main.rs"));
    }
}
