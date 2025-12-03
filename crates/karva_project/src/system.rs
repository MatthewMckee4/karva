use std::{
    error::Error,
    ffi::OsStr,
    fmt::Debug,
    io,
    path::{Path, PathBuf},
};

use camino::{Utf8Path, Utf8PathBuf};
use filetime::FileTime;
use glob::PatternError;

mod os;
pub mod walk_directory;

pub use os::OsSystem;
use walk_directory::WalkDirectoryBuilder;

pub type Result<T> = std::io::Result<T>;

/// The system on which Karva runs.
///
/// Karva supports running on the CLI.
///
/// Abstracting the system enables tests to use a more efficient in-memory file system.
pub trait System: Debug + Sync + Send {
    /// Reads the metadata of the file or directory at `path`.
    ///
    /// This function will traverse symbolic links to query information about the destination file.
    fn path_metadata(&self, path: &Utf8Path) -> Result<Metadata>;

    /// Returns the canonical, absolute form of a path with all intermediate components normalized
    /// and symbolic links resolved.
    ///
    /// # Errors
    /// This function will return an error in the following situations, but is not limited to just these cases:
    /// * `path` does not exist.
    /// * A non-final component in `path` is not a directory.
    /// * the symlink target path is not valid Unicode.
    ///
    /// ## Windows long-paths
    /// Unlike `std::fs::canonicalize`, this function does remove UNC prefixes if possible.
    /// See [`dunce::canonicalize`] for more information.
    fn canonicalize_path(&self, path: &Utf8Path) -> Result<Utf8PathBuf>;

    /// Returns the source type for `path` if known or `None`.
    ///
    /// The default is to always return `None`, assuming the system
    /// has no additional information and that the caller should
    /// rely on the file extension instead.
    ///
    /// This is primarily used for the LSP integration to respect
    /// the chosen language (or the fact that it is a notebook) in
    /// the editor.
    fn source_type(&self, path: &Utf8Path) -> Option<PySourceType> {
        let _ = path;
        None
    }

    /// Reads the content of the file at `path` into a [`String`].
    fn read_to_string(&self, path: &Utf8Path) -> Result<String>;

    /// Returns `true` if `path` exists.
    fn path_exists(&self, path: &Utf8Path) -> bool {
        self.path_metadata(path).is_ok()
    }

    /// Returns `true` if `path` exists and is a directory.
    fn is_directory(&self, path: &Utf8Path) -> bool {
        self.path_metadata(path)
            .is_ok_and(|metadata| metadata.file_type.is_directory())
    }

    /// Returns `true` if `path` exists and is a file.
    fn is_file(&self, path: &Utf8Path) -> bool {
        self.path_metadata(path)
            .is_ok_and(|metadata| metadata.file_type.is_file())
    }

    /// Returns the current working directory
    fn current_directory(&self) -> &Utf8Path;

    /// Returns the directory path where user configurations are stored.
    ///
    /// Returns `None` if no such convention exists for the system.
    fn user_config_directory(&self) -> Option<Utf8PathBuf>;

    /// Returns the directory path where cached files are stored.
    ///
    /// Returns `None` if no such convention exists for the system.
    fn cache_dir(&self) -> Option<Utf8PathBuf>;

    /// Iterate over the contents of the directory at `path`.
    ///
    /// The returned iterator must have the following properties:
    /// - It only iterates over the top level of the directory,
    ///   i.e., it does not recurse into subdirectories.
    /// - It skips the current and parent directories (`.` and `..`
    ///   respectively).
    /// - The iterator yields `std::io::Result<DirEntry>` instances.
    ///   For each instance, an `Err` variant may signify that the path
    ///   of the entry was not valid UTF8, in which case it should be an
    ///   [`std::io::Error`] with the `ErrorKind` set to
    ///   [`std::io::ErrorKind::InvalidData`] and the payload set to a
    ///   [`camino::FromPathBufError`]. It may also indicate that
    ///   "some sort of intermittent IO error occurred during iteration"
    ///   (language taken from the [`std::fs::read_dir`] documentation).
    ///
    /// # Errors
    /// Returns an error:
    /// - if `path` does not exist in the system,
    /// - if `path` does not point to a directory,
    /// - if the process does not have sufficient permissions to
    ///   view the contents of the directory at `path`
    /// - May also return an error in some other situations as well.
    fn read_directory<'a>(
        &'a self,
        path: &Utf8Path,
    ) -> Result<Box<dyn Iterator<Item = Result<DirectoryEntry>> + 'a>>;

    /// Recursively walks the content of `path`.
    ///
    /// It is allowed to pass a `path` that points to a file. In this case, the walker
    /// yields a single entry for that file.
    fn walk_directory(&self, path: &Utf8Path) -> WalkDirectoryBuilder;

    /// Return an iterator that produces all the `Path`s that match the given
    /// pattern using default match options, which may be absolute or relative to
    /// the current working directory.
    ///
    /// This may return an error if the pattern is invalid.
    fn glob(
        &self,
        pattern: &str,
    ) -> std::result::Result<
        Box<dyn Iterator<Item = std::result::Result<Utf8PathBuf, GlobError>> + '_>,
        PatternError,
    >;

    /// Fetches the environment variable `key` from the current process.
    ///
    /// # Errors
    ///
    /// Returns [`std::env::VarError::NotPresent`] if:
    /// - The variable is not set.
    /// - The variable's name contains an equal sign or NUL (`'='` or `'\0'`).
    ///
    /// Returns [`std::env::VarError::NotUnicode`] if the variable's value is not valid
    /// Unicode.
    fn env_var(&self, name: &str) -> std::result::Result<String, std::env::VarError> {
        let _ = name;
        Err(std::env::VarError::NotPresent)
    }

    /// Returns a handle to a [`WritableSystem`] if this system is writeable.
    fn as_writable(&self) -> Option<&dyn WritableSystem>;

    fn as_any(&self) -> &dyn std::any::Any;

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;

    fn dyn_clone(&self) -> Box<dyn System>;
}

/// System trait for non-readonly systems.
pub trait WritableSystem: System {
    /// Creates a file at the given path.
    ///
    /// Returns an error if the file already exists.
    fn create_new_file(&self, path: &Utf8Path) -> Result<()>;

    /// Writes the given content to the file at the given path.
    fn write_file(&self, path: &Utf8Path, content: &str) -> Result<()>;

    /// Creates a directory at `path` as well as any intermediate directories.
    fn create_directory_all(&self, path: &Utf8Path) -> Result<()>;

    /// Reads the provided file from the system cache, or creates the file if necessary.
    ///
    /// Returns `Ok(None)` if the system does not expose a suitable cache directory.
    fn get_or_cache(
        &self,
        path: &Utf8Path,
        read_contents: &dyn Fn() -> Result<String>,
    ) -> Result<Option<Utf8PathBuf>> {
        let Some(cache_dir) = self.cache_dir() else {
            return Ok(None);
        };

        let cache_path = cache_dir.join(path);

        // The file has already been cached.
        if self.is_file(&cache_path) {
            return Ok(Some(cache_path));
        }

        // Read the file contents.
        let contents = read_contents()?;

        // Create the parent directory.
        self.create_directory_all(cache_path.parent().unwrap())?;

        // Create and write to the file on the system.
        //
        // Note that `create_new_file` will fail if the file has already been created. This
        // ensures that only one thread/process ever attempts to write to it to avoid corrupting
        // the cache.
        self.create_new_file(&cache_path)?;
        self.write_file(&cache_path, &contents)?;

        Ok(Some(cache_path))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Metadata {
    revision: FileRevision,
    permissions: Option<u32>,
    file_type: FileType,
}

impl Metadata {
    pub const fn new(
        revision: FileRevision,
        permissions: Option<u32>,
        file_type: FileType,
    ) -> Self {
        Self {
            revision,
            permissions,
            file_type,
        }
    }

    pub const fn revision(&self) -> FileRevision {
        self.revision
    }

    pub const fn permissions(&self) -> Option<u32> {
        self.permissions
    }

    pub const fn file_type(&self) -> FileType {
        self.file_type
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash)]
pub enum FileType {
    File,
    Directory,
    Symlink,
}

impl FileType {
    pub const fn is_file(self) -> bool {
        matches!(self, Self::File)
    }

    pub const fn is_directory(self) -> bool {
        matches!(self, Self::Directory)
    }

    pub const fn is_symlink(self) -> bool {
        matches!(self, Self::Symlink)
    }
}

impl From<std::fs::FileType> for FileType {
    fn from(file_type: std::fs::FileType) -> Self {
        if file_type.is_file() {
            Self::File
        } else if file_type.is_dir() {
            Self::Directory
        } else {
            Self::Symlink
        }
    }
}

/// A number representing the revision of a file.
///
/// Two revisions that don't compare equal signify that the file has been modified.
/// Revisions aren't guaranteed to be monotonically increasing or in any specific order.
///
/// Possible revisions are:
/// * The last modification time of the file.
/// * The hash of the file's content.
/// * The revision as it comes from an external system, for example the LSP.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub struct FileRevision(u128);

impl FileRevision {
    pub const fn new(value: u128) -> Self {
        Self(value)
    }

    pub fn now() -> Self {
        Self::from(file_time_now())
    }

    pub const fn zero() -> Self {
        Self(0)
    }

    #[must_use]
    pub const fn as_u128(self) -> u128 {
        self.0
    }
}

impl From<u128> for FileRevision {
    fn from(value: u128) -> Self {
        Self(value)
    }
}

impl From<u64> for FileRevision {
    fn from(value: u64) -> Self {
        Self(u128::from(value))
    }
}

impl From<filetime::FileTime> for FileRevision {
    fn from(value: filetime::FileTime) -> Self {
        #[allow(clippy::cast_sign_loss)]
        let seconds = value.seconds() as u128;
        let seconds = seconds << 64;
        let nanos = u128::from(value.nanoseconds());

        Self(seconds | nanos)
    }
}

pub fn file_time_now() -> FileTime {
    FileTime::now()
}

#[derive(Debug, PartialEq, Eq)]
pub struct DirectoryEntry {
    path: Utf8PathBuf,
    file_type: FileType,
}

impl DirectoryEntry {
    pub const fn new(path: Utf8PathBuf, file_type: FileType) -> Self {
        Self { path, file_type }
    }

    pub fn into_path(self) -> Utf8PathBuf {
        self.path
    }

    pub fn path(&self) -> &Utf8Path {
        &self.path
    }

    pub const fn file_type(&self) -> FileType {
        self.file_type
    }
}

/// A glob iteration error.
///
/// This is typically returned when a particular path cannot be read
/// to determine if its contents match the glob pattern. This is possible
/// if the program lacks the appropriate permissions, for example.
#[derive(Debug)]
pub struct GlobError {
    path: PathBuf,
    error: GlobErrorKind,
}

impl GlobError {
    /// The Path that the error corresponds to.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub const fn kind(&self) -> &GlobErrorKind {
        &self.error
    }
}

impl Error for GlobError {}

impl std::fmt::Display for GlobError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match &self.error {
            GlobErrorKind::IOError(error) => {
                write!(
                    f,
                    "attempting to read `{}` resulted in an error: {error}",
                    self.path.display(),
                )
            }
            GlobErrorKind::NonUtf8Path => {
                write!(f, "`{}` is not a valid UTF-8 path", self.path.display(),)
            }
        }
    }
}

impl From<glob::GlobError> for GlobError {
    fn from(value: glob::GlobError) -> Self {
        Self {
            path: value.path().to_path_buf(),
            error: GlobErrorKind::IOError(value.into_error()),
        }
    }
}

#[derive(Debug)]
pub enum GlobErrorKind {
    IOError(io::Error),
    NonUtf8Path,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PySourceType {
    /// The source is a Python file (`.py`, `.pyw`).
    /// Note: `.pyw` files contain Python code, but do not represent importable namespaces.
    /// Consider adding a separate source type later if combining the two causes issues.
    #[default]
    Python,
    /// The source is a Python stub file (`.pyi`).
    Stub,
    /// The source is a Jupyter notebook (`.ipynb`).
    Ipynb,
}

impl PySourceType {
    /// Infers the source type from the file extension.
    ///
    /// Falls back to `Python` if the extension is not recognized.
    pub fn from_extension(extension: &str) -> Self {
        Self::try_from_extension(extension).unwrap_or_default()
    }

    /// Infers the source type from the file extension.
    pub fn try_from_extension(extension: &str) -> Option<Self> {
        let ty = match extension {
            "pyi" => Self::Stub,
            "py" | "pyw" => Self::Python,
            "ipynb" => Self::Ipynb,
            _ => return None,
        };

        Some(ty)
    }

    pub fn try_from_path(path: impl AsRef<Path>) -> Option<Self> {
        path.as_ref()
            .extension()
            .and_then(OsStr::to_str)
            .and_then(Self::try_from_extension)
    }

    pub const fn is_py_file(self) -> bool {
        matches!(self, Self::Python)
    }

    pub const fn is_stub(self) -> bool {
        matches!(self, Self::Stub)
    }

    pub const fn is_py_file_or_stub(self) -> bool {
        matches!(self, Self::Python | Self::Stub)
    }

    pub const fn is_ipynb(self) -> bool {
        matches!(self, Self::Ipynb)
    }
}

impl<P: AsRef<Path>> From<P> for PySourceType {
    fn from(path: P) -> Self {
        Self::try_from_path(path).unwrap_or_default()
    }
}
