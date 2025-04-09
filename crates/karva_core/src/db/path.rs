use camino::{Utf8Path, Utf8PathBuf};
use std::borrow::Borrow;
use std::fmt::Formatter;
use std::ops::Deref;
use std::path::{Path, PathBuf, StripPrefixError};

#[derive(Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct SystemPath(Utf8Path);

impl SystemPath {
    pub fn new(path: &(impl AsRef<Utf8Path> + ?Sized)) -> &Self {
        let path = path.as_ref();
        // SAFETY: FsPath is marked as #[repr(transparent)] so the conversion from a
        // *const Utf8Path to a *const FsPath is valid.
        unsafe { &*(path as *const Utf8Path as *const SystemPath) }
    }

    #[inline]
    #[must_use]
    pub fn extension(&self) -> Option<&str> {
        self.0.extension()
    }

    #[inline]
    #[must_use]
    pub fn starts_with(&self, base: impl AsRef<SystemPath>) -> bool {
        self.0.starts_with(base.as_ref())
    }

    #[inline]
    #[must_use]
    pub fn ends_with(&self, child: impl AsRef<SystemPath>) -> bool {
        self.0.ends_with(child.as_ref())
    }

    /// Returns the `FileSystemPath` without its final component, if there is one.
    ///
    /// Returns [`None`] if the path terminates in a root or prefix.
    ///
    /// # Examples
    ///
    /// ```
    /// use ruff_db::system::SystemPath;
    ///
    /// let path = SystemPath::new("/foo/bar");
    /// let parent = path.parent().unwrap();
    /// assert_eq!(parent, SystemPath::new("/foo"));
    ///
    /// let grand_parent = parent.parent().unwrap();
    /// assert_eq!(grand_parent, SystemPath::new("/"));
    /// assert_eq!(grand_parent.parent(), None);
    /// ```
    #[inline]
    #[must_use]
    pub fn parent(&self) -> Option<&SystemPath> {
        self.0.parent().map(SystemPath::new)
    }

    /// Produces an iterator over `SystemPath` and its ancestors.
    ///
    /// The iterator will yield the `SystemPath` that is returned if the [`parent`] method is used zero
    /// or more times. That means, the iterator will yield `&self`, `&self.parent().unwrap()`,
    /// `&self.parent().unwrap().parent().unwrap()` and so on. If the [`parent`] method returns
    /// [`None`], the iterator will do likewise. The iterator will always yield at least one value,
    /// namely `&self`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ruff_db::system::SystemPath;
    ///
    /// let mut ancestors = SystemPath::new("/foo/bar").ancestors();
    /// assert_eq!(ancestors.next(), Some(SystemPath::new("/foo/bar")));
    /// assert_eq!(ancestors.next(), Some(SystemPath::new("/foo")));
    /// assert_eq!(ancestors.next(), Some(SystemPath::new("/")));
    /// assert_eq!(ancestors.next(), None);
    ///
    /// let mut ancestors = SystemPath::new("../foo/bar").ancestors();
    /// assert_eq!(ancestors.next(), Some(SystemPath::new("../foo/bar")));
    /// assert_eq!(ancestors.next(), Some(SystemPath::new("../foo")));
    /// assert_eq!(ancestors.next(), Some(SystemPath::new("..")));
    /// assert_eq!(ancestors.next(), Some(SystemPath::new("")));
    /// assert_eq!(ancestors.next(), None);
    /// ```
    ///
    /// [`parent`]: SystemPath::parent
    #[inline]
    pub fn ancestors(&self) -> impl Iterator<Item = &SystemPath> {
        self.0.ancestors().map(SystemPath::new)
    }

    /// Produces an iterator over the [`camino::Utf8Component`]s of the path.
    ///
    /// When parsing the path, there is a small amount of normalization:
    ///
    /// * Repeated separators are ignored, so `a/b` and `a//b` both have
    ///   `a` and `b` as components.
    ///
    /// * Occurrences of `.` are normalized away, except if they are at the
    ///   beginning of the path. For example, `a/./b`, `a/b/`, `a/b/.` and
    ///   `a/b` all have `a` and `b` as components, but `./a/b` starts with
    ///   an additional [`CurDir`] component.
    ///
    /// * A trailing slash is normalized away, `/a/b` and `/a/b/` are equivalent.
    ///
    /// Note that no other normalization takes place; in particular, `a/c`
    /// and `a/b/../c` are distinct, to account for the possibility that `b`
    /// is a symbolic link (so its parent isn't `a`).
    ///
    /// # Examples
    ///
    /// ```
    /// use camino::{Utf8Component};
    /// use ruff_db::system::SystemPath;
    ///
    /// let mut components = SystemPath::new("/tmp/foo.txt").components();
    ///
    /// assert_eq!(components.next(), Some(Utf8Component::RootDir));
    /// assert_eq!(components.next(), Some(Utf8Component::Normal("tmp")));
    /// assert_eq!(components.next(), Some(Utf8Component::Normal("foo.txt")));
    /// assert_eq!(components.next(), None)
    /// ```
    ///
    /// [`CurDir`]: camino::Utf8Component::CurDir
    #[inline]
    pub fn components(&self) -> camino::Utf8Components {
        self.0.components()
    }

    /// Returns the final component of the `FileSystemPath`, if there is one.
    ///
    /// If the path is a normal file, this is the file name. If it's the path of a directory, this
    /// is the directory name.
    ///
    /// Returns [`None`] if the path terminates in `..`.
    ///
    /// # Examples
    ///
    /// ```
    /// use camino::Utf8Path;
    /// use ruff_db::system::SystemPath;
    ///
    /// assert_eq!(Some("bin"), SystemPath::new("/usr/bin/").file_name());
    /// assert_eq!(Some("foo.txt"), SystemPath::new("tmp/foo.txt").file_name());
    /// assert_eq!(Some("foo.txt"), SystemPath::new("foo.txt/.").file_name());
    /// assert_eq!(Some("foo.txt"), SystemPath::new("foo.txt/.//").file_name());
    /// assert_eq!(None, SystemPath::new("foo.txt/..").file_name());
    /// assert_eq!(None, SystemPath::new("/").file_name());
    /// ```
    #[inline]
    #[must_use]
    pub fn file_name(&self) -> Option<&str> {
        self.0.file_name()
    }

    /// Extracts the stem (non-extension) portion of [`self.file_name`].
    ///
    /// [`self.file_name`]: SystemPath::file_name
    ///
    /// The stem is:
    ///
    /// * [`None`], if there is no file name;
    /// * The entire file name if there is no embedded `.`;
    /// * The entire file name if the file name begins with `.` and has no other `.`s within;
    /// * Otherwise, the portion of the file name before the final `.`
    ///
    /// # Examples
    ///
    /// ```
    /// use ruff_db::system::SystemPath;
    ///
    /// assert_eq!("foo", SystemPath::new("foo.rs").file_stem().unwrap());
    /// assert_eq!("foo.tar", SystemPath::new("foo.tar.gz").file_stem().unwrap());
    /// ```
    #[inline]
    #[must_use]
    pub fn file_stem(&self) -> Option<&str> {
        self.0.file_stem()
    }

    /// Returns a path that, when joined onto `base`, yields `self`.
    ///
    /// # Errors
    ///
    /// If `base` is not a prefix of `self` (i.e., [`starts_with`]
    /// returns `false`), returns [`Err`].
    ///
    /// [`starts_with`]: SystemPath::starts_with
    ///
    /// # Examples
    ///
    /// ```
    /// use ruff_db::system::{SystemPath, SystemPathBuf};
    ///
    /// let path = SystemPath::new("/test/haha/foo.txt");
    ///
    /// assert_eq!(path.strip_prefix("/"), Ok(SystemPath::new("test/haha/foo.txt")));
    /// assert_eq!(path.strip_prefix("/test"), Ok(SystemPath::new("haha/foo.txt")));
    /// assert_eq!(path.strip_prefix("/test/"), Ok(SystemPath::new("haha/foo.txt")));
    /// assert_eq!(path.strip_prefix("/test/haha/foo.txt"), Ok(SystemPath::new("")));
    /// assert_eq!(path.strip_prefix("/test/haha/foo.txt/"), Ok(SystemPath::new("")));
    ///
    /// assert!(path.strip_prefix("test").is_err());
    /// assert!(path.strip_prefix("/haha").is_err());
    ///
    /// let prefix = SystemPathBuf::from("/test/");
    /// assert_eq!(path.strip_prefix(prefix), Ok(SystemPath::new("haha/foo.txt")));
    /// ```
    #[inline]
    pub fn strip_prefix(
        &self,
        base: impl AsRef<SystemPath>,
    ) -> std::result::Result<&SystemPath, StripPrefixError> {
        self.0.strip_prefix(base.as_ref()).map(SystemPath::new)
    }

    /// Creates an owned [`SystemPathBuf`] with `path` adjoined to `self`.
    ///
    /// See [`std::path::PathBuf::push`] for more details on what it means to adjoin a path.
    ///
    /// # Examples
    ///
    /// ```
    /// use ruff_db::system::{SystemPath, SystemPathBuf};
    ///
    /// assert_eq!(SystemPath::new("/etc").join("passwd"), SystemPathBuf::from("/etc/passwd"));
    /// ```
    #[inline]
    #[must_use]
    pub fn join(&self, path: impl AsRef<SystemPath>) -> SystemPathBuf {
        SystemPathBuf::from_utf8_path_buf(self.0.join(&path.as_ref().0))
    }

    /// Creates an owned [`SystemPathBuf`] like `self` but with the given extension.
    ///
    /// See [`std::path::PathBuf::set_extension`] for more details.
    ///
    /// # Examples
    ///
    /// ```
    /// use ruff_db::system::{SystemPath, SystemPathBuf};
    ///
    /// let path = SystemPath::new("foo.rs");
    /// assert_eq!(path.with_extension("txt"), SystemPathBuf::from("foo.txt"));
    ///
    /// let path = SystemPath::new("foo.tar.gz");
    /// assert_eq!(path.with_extension(""), SystemPathBuf::from("foo.tar"));
    /// assert_eq!(path.with_extension("xz"), SystemPathBuf::from("foo.tar.xz"));
    /// assert_eq!(path.with_extension("").with_extension("txt"), SystemPathBuf::from("foo.txt"));
    /// ```
    #[inline]
    pub fn with_extension(&self, extension: &str) -> SystemPathBuf {
        SystemPathBuf::from_utf8_path_buf(self.0.with_extension(extension))
    }

    /// Converts the path to an owned [`SystemPathBuf`].
    pub fn to_path_buf(&self) -> SystemPathBuf {
        SystemPathBuf(self.0.to_path_buf())
    }

    /// Returns the path as a string slice.
    #[inline]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Returns the std path for the file.
    #[inline]
    pub fn as_std_path(&self) -> &Path {
        self.0.as_std_path()
    }

    /// Returns the [`Utf8Path`] for the file.
    #[inline]
    pub fn as_utf8_path(&self) -> &Utf8Path {
        &self.0
    }

    pub fn from_std_path(path: &Path) -> Option<&SystemPath> {
        Some(SystemPath::new(Utf8Path::from_path(path)?))
    }

    pub fn absolute(path: impl AsRef<SystemPath>, cwd: impl AsRef<SystemPath>) -> SystemPathBuf {
        fn absolute(path: &SystemPath, cwd: &SystemPath) -> SystemPathBuf {
            let path = &path.0;

            let mut components = path.components().peekable();
            let mut ret = if let Some(
                c @ (camino::Utf8Component::Prefix(..) | camino::Utf8Component::RootDir),
            ) = components.peek().cloned()
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
}

impl ToOwned for SystemPath {
    type Owned = SystemPathBuf;

    fn to_owned(&self) -> Self::Owned {
        self.to_path_buf()
    }
}

#[repr(transparent)]
#[derive(Eq, PartialEq, Clone, Hash, PartialOrd, Ord)]
pub struct SystemPathBuf(Utf8PathBuf);

impl SystemPathBuf {
    pub fn new() -> Self {
        Self(Utf8PathBuf::new())
    }

    pub fn from_utf8_path_buf(path: Utf8PathBuf) -> Self {
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

    pub fn into_utf8_path_buf(self) -> Utf8PathBuf {
        self.0
    }

    pub fn into_std_path_buf(self) -> PathBuf {
        self.0.into_std_path_buf()
    }

    #[inline]
    pub fn as_path(&self) -> &SystemPath {
        SystemPath::new(&self.0)
    }
}

impl Borrow<SystemPath> for SystemPathBuf {
    fn borrow(&self) -> &SystemPath {
        self.as_path()
    }
}

impl From<&str> for SystemPathBuf {
    fn from(value: &str) -> Self {
        SystemPathBuf::from_utf8_path_buf(Utf8PathBuf::from(value))
    }
}

impl From<String> for SystemPathBuf {
    fn from(value: String) -> Self {
        SystemPathBuf::from_utf8_path_buf(Utf8PathBuf::from(value))
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

impl AsRef<SystemPath> for SystemPath {
    #[inline]
    fn as_ref(&self) -> &SystemPath {
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
