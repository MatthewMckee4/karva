use std::sync::Arc;
use std::{fmt::Debug, num::NonZeroUsize};

use anyhow::Context;
use camino::{Utf8Path, Utf8PathBuf};
use filetime::FileTime;
use karva_static::EnvVars;
use ruff_db::system::{FileType, Metadata};

pub mod path;
pub mod time;

type Result<T> = std::io::Result<T>;

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

pub fn max_parallelism() -> NonZeroUsize {
    std::env::var(EnvVars::KARVA_MAX_PARALLELISM)
        .or_else(|_| std::env::var(EnvVars::RAYON_NUM_THREADS))
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            std::thread::available_parallelism().unwrap_or_else(|_| NonZeroUsize::new(1).unwrap())
        })
}

/// Find the karva wheel in the target/wheels directory.
/// Returns the path to the wheel file.
pub fn find_karva_wheel() -> anyhow::Result<Utf8PathBuf> {
    let karva_root = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .ok_or_else(|| anyhow::anyhow!("Could not determine KARVA_ROOT"))?
        .to_path_buf();

    let wheels_dir = karva_root.join("target").join("wheels");

    let entries = std::fs::read_dir(&wheels_dir)
        .with_context(|| format!("Could not read wheels directory: {wheels_dir}"))?;

    for entry in entries {
        let entry = entry?;
        let file_name = entry.file_name();
        if let Some(name) = file_name.to_str() {
            if name.starts_with("karva-")
                && Utf8Path::new(name)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("whl"))
            {
                return Ok(
                    Utf8PathBuf::from_path_buf(entry.path()).expect("Path is not valid UTF-8")
                );
            }
        }
    }

    anyhow::bail!("Could not find karva wheel in target/wheels directory");
}

/// Find the cargo binary in the target directory.
/// Returns the path to the binary file.
pub fn find_cargo_binary(binary: &str) -> anyhow::Result<Utf8PathBuf> {
    let karva_root = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .ok_or_else(|| anyhow::anyhow!("Could not determine KARVA_ROOT"))?
        .to_path_buf();

    let debug_dir = karva_root.join("target").join("debug");

    let entries = std::fs::read_dir(&debug_dir)
        .with_context(|| format!("Could not read target directory: {debug_dir}"))?;

    for entry in entries {
        let entry = entry?;
        let file_name = entry.file_name();
        if let Some(name) = file_name.to_str() {
            if name.starts_with(binary) {
                return Ok(
                    Utf8PathBuf::from_path_buf(entry.path()).expect("Path is not valid UTF-8")
                );
            }
        }
    }

    anyhow::bail!("Could not find karva wheel in target/wheels directory");
}

pub fn venv_binary(binary_name: &str, directory: &Utf8PathBuf) -> Option<Utf8PathBuf> {
    let venv_dir = directory.join(".venv");

    let binary_dir = if cfg!(target_os = "windows") {
        venv_dir.join("Scripts")
    } else {
        venv_dir.join("bin")
    };

    let binary_path = if cfg!(target_os = "windows") {
        binary_dir.join(format!("{binary_name}.exe"))
    } else {
        binary_dir.join(binary_name)
    };

    if binary_path.exists() {
        Some(binary_path)
    } else {
        None
    }
}
