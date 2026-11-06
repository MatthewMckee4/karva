//! Filesystem boundary used by project discovery and source indexing.

use std::fmt::Debug;
use std::io;
use std::path::PathBuf;

use camino::{Utf8Path, Utf8PathBuf};
use ignore::{
    WalkBuilder,
    types::{Types, TypesBuilder},
};
use karva_metadata::{OsProjectFileSystem, ProjectFileMetadata, ProjectFileSystem};
use thiserror::Error;

/// File kind needed while selecting workspace sources.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileType {
    File,
    Directory,
    Symlink,
    Other,
}

impl FileType {
    pub const fn is_file(self) -> bool {
        matches!(self, Self::File)
    }

    #[cfg(test)]
    const fn is_directory(self) -> bool {
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
        } else if file_type.is_symlink() {
            Self::Symlink
        } else {
            Self::Other
        }
    }
}

/// One owned entry returned by a directory walk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryEntry {
    path: Utf8PathBuf,
    file_type: FileType,
}

impl DirectoryEntry {
    pub fn path(&self) -> &Utf8Path {
        &self.path
    }

    pub const fn file_type(&self) -> FileType {
        self.file_type
    }
}

/// Directory-walk behavior required by source discovery.
#[derive(Clone, Copy, Debug)]
pub struct WalkOptions {
    /// Honor project ignore rules; hidden directories and symlinks remain excluded.
    pub respect_ignore_files: bool,

    /// Maximum depth below the root, or an unrestricted recursive walk.
    pub max_depth: Option<usize>,
}

/// Failure to enumerate a filesystem path through [`System`].
#[derive(Debug, Error)]
pub enum SystemError {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error("filesystem path is not valid UTF-8: {0:?}")]
    NonUtf8Path(PathBuf),
}

/// Narrow host boundary shared by project discovery and source indexing.
pub trait System: ProjectFileSystem + Debug + Send + Sync {
    /// Reads the host working directory without changing process state.
    fn current_directory(&self) -> io::Result<PathBuf>;

    /// Returns immediate child paths; failures retain their operating-system kind.
    fn read_directory(&self, path: &Utf8Path) -> io::Result<Vec<Utf8PathBuf>>;

    /// Expands configured selections using the same literal-first rules as the CLI.
    fn resolve_test_paths(
        &self,
        value: &str,
    ) -> Vec<Result<karva_project::path::TestPath, karva_project::path::TestPathError>>;

    /// Inspects the entry itself, without following a symlink.
    fn symlink_path_metadata(&self, path: &Utf8Path) -> io::Result<FileType>;

    /// Lazily enumerates regular Python files, preserving the CLI type-filter rules.
    /// Callers can cancel between filesystem operations; directory entries are skipped.
    fn walk_python_files(
        &self,
        root: &Utf8Path,
        options: WalkOptions,
    ) -> Box<dyn Iterator<Item = Result<DirectoryEntry, SystemError>> + Send>;
}

/// Native operating-system implementation used by the language server.
#[derive(Clone, Copy, Debug, Default)]
pub struct OsSystem;

impl ProjectFileSystem for OsSystem {
    fn path_metadata(&self, path: &Utf8Path) -> io::Result<ProjectFileMetadata> {
        OsProjectFileSystem.path_metadata(path)
    }

    fn read_to_string(&self, path: &Utf8Path) -> io::Result<String> {
        OsProjectFileSystem.read_to_string(path)
    }
}

impl System for OsSystem {
    fn resolve_test_paths(
        &self,
        value: &str,
    ) -> Vec<Result<karva_project::path::TestPath, karva_project::path::TestPathError>> {
        karva_project::path::resolve_test_paths(value)
    }

    fn read_directory(&self, path: &Utf8Path) -> io::Result<Vec<Utf8PathBuf>> {
        Ok(std::fs::read_dir(path)?
            .flatten()
            .filter_map(|entry| Utf8PathBuf::from_path_buf(entry.path()).ok())
            .collect())
    }

    fn current_directory(&self) -> io::Result<PathBuf> {
        std::env::current_dir()
    }

    fn symlink_path_metadata(&self, path: &Utf8Path) -> io::Result<FileType> {
        std::fs::symlink_metadata(path).map(|metadata| metadata.file_type().into())
    }

    fn walk_python_files(
        &self,
        root: &Utf8Path,
        options: WalkOptions,
    ) -> Box<dyn Iterator<Item = Result<DirectoryEntry, SystemError>> + Send> {
        let WalkOptions {
            respect_ignore_files,
            max_depth,
        } = options;
        let types = match python_file_types() {
            Ok(types) => types,
            Err(error) => {
                return Box::new(std::iter::once(Err(SystemError::Io(io::Error::other(
                    error,
                )))));
            }
        };
        let mut builder = WalkBuilder::new(root);
        builder
            .standard_filters(true)
            .parents(true)
            .git_ignore(respect_ignore_files)
            .git_global(false)
            .require_git(false)
            .follow_links(false)
            .max_depth(max_depth)
            .types(types);
        let entries = builder.build().filter_map(|entry| {
            let entry = match entry {
                Ok(entry) => entry,
                Err(source) => return Some(Err(SystemError::Io(io::Error::other(source)))),
            };
            if !entry
                .file_type()
                .is_some_and(|file_type| file_type.is_file())
            {
                return None;
            }
            Some(
                Utf8PathBuf::from_path_buf(entry.into_path())
                    .map_err(SystemError::NonUtf8Path)
                    .map(|path| DirectoryEntry {
                        path,
                        file_type: FileType::File,
                    }),
            )
        });
        Box::new(entries)
    }
}

fn python_file_types() -> Result<Types, ignore::Error> {
    let mut types = TypesBuilder::new();
    types.add("python", "*.py")?;
    types.select("python");
    types.build()
}

#[cfg(test)]
pub use memory::MemorySystem;

#[cfg(test)]
mod memory {
    use std::collections::BTreeMap;

    use super::*;

    /// Deterministic filesystem used by language-server unit tests.
    #[derive(Debug)]
    pub struct MemorySystem {
        current_directory: Utf8PathBuf,
        current_directory_error: Option<io::ErrorKind>,
        entries: BTreeMap<Utf8PathBuf, FileType>,
        files: BTreeMap<Utf8PathBuf, String>,
        metadata_errors: BTreeMap<Utf8PathBuf, io::ErrorKind>,
        read_errors: BTreeMap<Utf8PathBuf, io::ErrorKind>,
        walk_errors: BTreeMap<Utf8PathBuf, io::ErrorKind>,
    }

    impl MemorySystem {
        pub(crate) fn new(current_directory: impl Into<Utf8PathBuf>) -> Self {
            Self {
                current_directory: current_directory.into(),
                current_directory_error: None,
                entries: BTreeMap::new(),
                files: BTreeMap::new(),
                metadata_errors: BTreeMap::new(),
                read_errors: BTreeMap::new(),
                walk_errors: BTreeMap::new(),
            }
        }

        pub(crate) fn add_directory(&mut self, path: &Utf8Path) {
            let path = self.resolve(path);
            self.add_parent_directories(&path);
            self.entries.insert(path, FileType::Directory);
        }

        pub(crate) fn add_file(&mut self, path: &Utf8Path, contents: impl Into<String>) {
            let path = self.resolve(path);
            self.add_parent_directories(&path);
            self.entries.insert(path.clone(), FileType::File);
            self.files.insert(path, contents.into());
        }

        pub(crate) fn add_symlink(&mut self, path: &Utf8Path) {
            let path = self.resolve(path);
            self.add_parent_directories(&path);
            self.entries.insert(path, FileType::Symlink);
        }

        pub(crate) fn fail_current_directory(&mut self, kind: io::ErrorKind) {
            self.current_directory_error = Some(kind);
        }

        pub(crate) fn fail_metadata(&mut self, path: &Utf8Path, kind: io::ErrorKind) {
            let path = self.resolve(path);
            self.metadata_errors.insert(path, kind);
        }

        pub(crate) fn fail_read(&mut self, path: &Utf8Path, kind: io::ErrorKind) {
            let path = self.resolve(path);
            self.read_errors.insert(path, kind);
        }

        pub(crate) fn fail_walk(&mut self, path: &Utf8Path, kind: io::ErrorKind) {
            let path = self.resolve(path);
            self.walk_errors.insert(path, kind);
        }

        fn add_parent_directories(&mut self, path: &Utf8Path) {
            for parent in path.parent().into_iter().flat_map(Utf8Path::ancestors) {
                self.entries
                    .entry(parent.to_path_buf())
                    .or_insert(FileType::Directory);
            }
        }

        fn error(kind: io::ErrorKind, path: &Utf8Path) -> io::Error {
            io::Error::new(kind, format!("{path}: {kind}"))
        }

        fn resolve(&self, path: &Utf8Path) -> Utf8PathBuf {
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                self.current_directory.join(path)
            }
        }
    }

    impl ProjectFileSystem for MemorySystem {
        fn path_metadata(&self, path: &Utf8Path) -> io::Result<ProjectFileMetadata> {
            let path = self.resolve(path);
            if let Some(kind) = self.metadata_errors.get(&path) {
                return Err(Self::error(*kind, &path));
            }
            self.entries
                .get(&path)
                .copied()
                .map(|file_type| {
                    ProjectFileMetadata::new(file_type.is_file(), file_type.is_directory())
                })
                .ok_or_else(|| Self::error(io::ErrorKind::NotFound, &path))
        }

        fn read_to_string(&self, path: &Utf8Path) -> io::Result<String> {
            let path = self.resolve(path);
            if let Some(kind) = self.read_errors.get(&path) {
                return Err(Self::error(*kind, &path));
            }
            self.files
                .get(&path)
                .cloned()
                .ok_or_else(|| Self::error(io::ErrorKind::NotFound, &path))
        }
    }

    impl System for MemorySystem {
        fn resolve_test_paths(
            &self,
            value: &str,
        ) -> Vec<Result<karva_project::path::TestPath, karva_project::path::TestPathError>>
        {
            use karva_project::path::{TestPath, TestPathError};
            let literal = TestPath::new_with_file_system(value, self);
            if literal.is_ok() || value.contains("::") || !value.contains(['*', '?', '[']) {
                return vec![literal];
            }
            let pattern = match glob::Pattern::new(value) {
                Ok(pattern) => pattern,
                Err(error) => {
                    return vec![Err(TestPathError::InvalidGlob {
                        pattern: value.to_owned(),
                        error: error.to_string(),
                    })];
                }
            };
            let options = glob::MatchOptions {
                case_sensitive: true,
                require_literal_separator: true,
                require_literal_leading_dot: true,
            };
            let paths = self
                .entries
                .keys()
                .filter(|path| pattern.matches_with(path.as_str(), options))
                .map(|path| TestPath::new_with_file_system(path.as_str(), self))
                .collect::<Vec<_>>();
            if paths.is_empty() {
                vec![literal]
            } else {
                paths
            }
        }

        fn read_directory(&self, path: &Utf8Path) -> io::Result<Vec<Utf8PathBuf>> {
            let root = self.resolve(path);
            if !self
                .entries
                .get(&root)
                .is_some_and(|kind| kind.is_directory())
            {
                return Err(Self::error(io::ErrorKind::NotFound, &root));
            }
            Ok(self
                .entries
                .keys()
                .filter(|path| path.parent() == Some(root.as_path()))
                .cloned()
                .collect())
        }

        fn current_directory(&self) -> io::Result<PathBuf> {
            if let Some(kind) = self.current_directory_error {
                return Err(io::Error::new(kind, "current directory unavailable"));
            }
            Ok(self.current_directory.clone().into_std_path_buf())
        }

        fn symlink_path_metadata(&self, path: &Utf8Path) -> io::Result<FileType> {
            let path = self.resolve(path);
            if let Some(kind) = self.metadata_errors.get(&path) {
                return Err(Self::error(*kind, &path));
            }
            self.entries
                .get(&path)
                .copied()
                .ok_or_else(|| Self::error(io::ErrorKind::NotFound, &path))
        }

        fn walk_python_files(
            &self,
            root: &Utf8Path,
            options: WalkOptions,
        ) -> Box<dyn Iterator<Item = Result<DirectoryEntry, SystemError>> + Send> {
            let root = self.resolve(root);
            if let Some(kind) = self.walk_errors.get(&root) {
                let error = Self::error(*kind, &root);
                return Box::new(std::iter::once(Err(SystemError::Io(error))));
            }
            if !self
                .entries
                .get(&root)
                .is_some_and(|file_type| file_type.is_directory())
            {
                let error = Self::error(io::ErrorKind::NotFound, &root);
                return Box::new(std::iter::once(Err(SystemError::Io(error))));
            }

            let WalkOptions {
                respect_ignore_files: _,
                max_depth,
            } = options;
            let root_depth = root.components().count();
            let mut entries = Vec::new();
            for (path, file_type) in &self.entries {
                let relative = path.strip_prefix(&root);
                let Some(relative) = relative.ok() else {
                    continue;
                };
                if !file_type.is_file() || path.extension() != Some("py") {
                    continue;
                }
                let is_hidden = relative.parent().is_some_and(|parent| {
                    parent
                        .components()
                        .any(|component| component.as_str().starts_with('.'))
                });
                let is_symlink = path
                    .ancestors()
                    .skip(1)
                    .take_while(|ancestor| ancestor.starts_with(&root))
                    .any(|ancestor| self.entries.get(ancestor) == Some(&FileType::Symlink));
                let exceeds_depth = max_depth.is_some_and(|maximum| {
                    path.components().count().saturating_sub(root_depth) > maximum
                });
                if is_hidden || is_symlink || exceeds_depth {
                    continue;
                }
                entries.push(Ok(DirectoryEntry {
                    path: path.clone(),
                    file_type: *file_type,
                }));
            }
            Box::new(entries.into_iter())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    fn python_type_matches_keep_hidden_files_but_not_hidden_directories(
        #[values(false, true)] memory: bool,
    ) -> anyhow::Result<()> {
        let temporary = tempfile::tempdir()?;
        let root = Utf8PathBuf::from_path_buf(temporary.path().to_path_buf()).expect("UTF-8 root");
        let mut virtual_files = MemorySystem::new(root.clone());
        for relative in [
            ".test_hidden.py",
            "visible.py",
            ".hidden/ignored.py",
            "image.svg",
        ] {
            let path = root.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, "")?;
            virtual_files.add_file(&path, "");
        }
        let system: &dyn System = if memory { &virtual_files } else { &OsSystem };
        let entries = system
            .walk_python_files(
                &root,
                WalkOptions {
                    respect_ignore_files: true,
                    max_depth: None,
                },
            )
            .collect::<Result<Vec<_>, _>>()?;
        let mut paths = entries
            .iter()
            .map(|entry| entry.path().strip_prefix(&root).map(Utf8Path::as_str))
            .collect::<Result<Vec<_>, _>>()?;
        paths.sort_unstable();
        assert_eq!(paths, [".test_hidden.py", "visible.py"]);
        Ok(())
    }

    // APFS rejects this fixture at creation; Linux accepts arbitrary filename bytes.
    #[cfg(target_os = "linux")]
    #[test]
    fn non_python_files_and_directories_need_not_have_utf8_names() -> anyhow::Result<()> {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let temporary = tempfile::tempdir()?;
        let root = Utf8PathBuf::from_path_buf(temporary.path().to_path_buf()).expect("UTF-8 root");
        let directory = temporary
            .path()
            .join(OsString::from_vec(b"directory-\xff".to_vec()));
        std::fs::create_dir(&directory)?;
        std::fs::write(directory.join("image.png"), "")?;
        std::fs::write(
            temporary
                .path()
                .join(OsString::from_vec(b"image-\xff.png".to_vec())),
            "",
        )?;
        let entries = OsSystem
            .walk_python_files(
                &root,
                WalkOptions {
                    respect_ignore_files: true,
                    max_depth: None,
                },
            )
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(entries, []);
        Ok(())
    }
}
