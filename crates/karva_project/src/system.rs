use std::{fmt::Debug, sync::Arc};

use camino::{Utf8Path, Utf8PathBuf};
use filetime::FileTime;
use ruff_db::system::{FileType, Metadata};

pub type Result<T> = std::io::Result<T>;

pub trait System: Debug + Sync + Send {
    /// Reads the metadata of the file or directory at `path`.
    ///
    /// This function will traverse symbolic links to query information about the destination file.
    fn path_metadata(&self, path: &Utf8Path) -> Result<Metadata>;

    fn read_to_string(&self, path: &Utf8Path) -> Result<String>;

    /// Returns the directory path where user configurations are stored.
    ///
    /// Returns `None` if no such convention exists for the system.
    fn user_config_directory(&self) -> Option<Utf8PathBuf>;

    fn current_directory(&self) -> &Utf8Path;

    /// Returns `true` if `path` exists and is a directory.
    fn is_directory(&self, path: &Utf8Path) -> bool {
        self.path_metadata(path)
            .is_ok_and(|metadata| metadata.file_type().is_directory())
    }
}

/// A system implementation that uses the OS file system.
#[derive(Debug, Clone)]
pub struct OsSystem {
    inner: Arc<OsSystemInner>,
}

#[derive(Default, Debug)]
struct OsSystemInner {
    cwd: Utf8PathBuf,
}

impl OsSystem {
    pub fn new(cwd: impl AsRef<Utf8Path>) -> Self {
        let cwd = cwd.as_ref();
        assert!(cwd.is_absolute());

        tracing::debug!(
            "Architecture: {}, OS: {}",
            std::env::consts::ARCH,
            std::env::consts::OS,
        );

        Self {
            inner: Arc::new(OsSystemInner {
                cwd: cwd.to_path_buf(),
            }),
        }
    }

    #[cfg(unix)]
    #[allow(clippy::unnecessary_wraps)]
    fn permissions(metadata: &std::fs::Metadata) -> Option<u32> {
        use std::os::unix::fs::PermissionsExt;

        Some(metadata.permissions().mode())
    }

    #[cfg(not(unix))]
    fn permissions(_metadata: &std::fs::Metadata) -> Option<u32> {
        None
    }
}

impl System for OsSystem {
    fn path_metadata(&self, path: &Utf8Path) -> Result<Metadata> {
        let metadata = path.as_std_path().metadata()?;
        let last_modified = FileTime::from_last_modification_time(&metadata);

        let file_type = if metadata.file_type().is_file() {
            FileType::File
        } else if metadata.file_type().is_dir() {
            FileType::Directory
        } else {
            FileType::Symlink
        };

        Ok(Metadata::new(
            last_modified.into(),
            Self::permissions(&metadata),
            file_type,
        ))
    }

    fn read_to_string(&self, path: &Utf8Path) -> Result<String> {
        std::fs::read_to_string(path)
    }

    fn user_config_directory(&self) -> Option<Utf8PathBuf> {
        use etcetera::BaseStrategy as _;

        let strategy = etcetera::base_strategy::choose_base_strategy().ok()?;
        strategy.config_dir().try_into().ok()
    }

    fn current_directory(&self) -> &Utf8Path {
        &self.inner.cwd
    }
}
