#![allow(clippy::print_stderr)]
use std::fs::File;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Instant;

use anyhow::Context;
use camino::{Utf8Path, Utf8PathBuf};
use directories::ProjectDirs;
use fs4::fs_std::FileExt;
use insta::Settings;
use insta::internals::SettingsBindDropGuard;
use tempfile::TempDir;

/// Lazily initialized shared venv path for test reuse (within single process).
/// Only used on Unix where symlinks work properly with venvs.
#[cfg(unix)]
static SHARED_VENV: OnceLock<Utf8PathBuf> = OnceLock::new();

pub struct TestContext {
    _temp_dir: TempDir,
    project_dir_path: Utf8PathBuf,
    venv_path: Utf8PathBuf,
    _settings_scope: SettingsBindDropGuard,
}

impl TestContext {
    pub fn new() -> Self {
        let start = Instant::now();
        let cache_dir = get_test_cache_dir();

        std::fs::create_dir_all(&cache_dir).expect("Failed to create cache directory");

        let temp_dir =
            TempDir::new_in(&cache_dir).expect("Failed to create temp directory in cache");

        let project_path = Utf8PathBuf::from_path_buf(
            dunce::simplified(
                &temp_dir
                    .path()
                    .canonicalize()
                    .context("Failed to canonicalize project path")
                    .unwrap(),
            )
            .to_path_buf(),
        )
        .expect("Path is not valid UTF-8");

        let python_version = std::env::var("PYTHON_VERSION").unwrap_or_else(|_| "3.13".to_string());

        let karva_wheel = karva_system::find_karva_wheel()
            .expect("Could not find karva wheel. Run `maturin build` before running tests.")
            .to_string();

        // Platform-specific venv setup:
        // - Unix: Use shared venv with symlinks for speed
        // - Windows: Create per-test venv (symlinks don't work properly with venvs on Windows)
        let venv_path = setup_venv(&cache_dir, &project_path, &python_version, &karva_wheel);

        let mut settings = Settings::clone_current();

        settings.add_filter(&tempdir_filter(&project_path), "<temp_dir>/");
        settings.add_filter(r#"\\(\w\w|\s|\.|")"#, "/$1");
        settings.add_filter(r"\x1b\[[0-9;]*m", "");
        settings.add_filter(r"(\s|\()(\d+m )?(\d+\.)?\d+(ms|s)", "$1[TIME]");

        let settings_scope = settings.bind_to_scope();

        eprintln!("Time to set up test context: {:?}", start.elapsed());

        Self {
            project_dir_path: project_path,
            venv_path,
            _temp_dir: temp_dir,
            _settings_scope: settings_scope,
        }
    }

    pub fn root(&self) -> Utf8PathBuf {
        self.project_dir_path.clone()
    }

    pub fn with_files<'a>(files: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let case = Self::default();
        case.write_files(files);
        case
    }

    pub fn with_file(path: impl AsRef<Utf8Path>, content: &str) -> Self {
        let case = Self::default();
        case.write_file(path, content);
        case
    }

    pub fn write_files<'a>(&self, files: impl IntoIterator<Item = (&'a str, &'a str)>) {
        for (path, content) in files {
            self.write_file(path, content);
        }
    }

    pub fn write_file(&self, path: impl AsRef<Utf8Path>, content: &str) {
        let path = path.as_ref();

        let path = self.project_dir_path.join(path);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory `{parent}`"))
                .unwrap();
        }

        std::fs::write(&path, &*ruff_python_trivia::textwrap::dedent(content))
            .with_context(|| format!("Failed to write file `{path}`"))
            .unwrap();
    }

    fn venv_binary(&self, binary: &str) -> Utf8PathBuf {
        self.venv_path
            .join(if cfg!(windows) { "Scripts" } else { "bin" })
            .join(if cfg!(windows) {
                format!("{binary}.exe")
            } else {
                binary.to_string()
            })
    }

    pub fn command(&self) -> Command {
        let mut command = Command::new(self.venv_binary("karva"));
        command.arg("test").current_dir(self.root());
        command
    }

    pub fn command_no_parallel(&self) -> Command {
        let mut command = self.command();
        command.arg("--no-parallel");
        command
    }
}

impl Default for TestContext {
    fn default() -> Self {
        Self::new()
    }
}

pub fn tempdir_filter(path: &Utf8Path) -> String {
    format!(r"{}\\?/?", regex::escape(path.as_str()))
}

// Use user cache directory so we can use `uv` caching.
pub fn get_test_cache_dir() -> Utf8PathBuf {
    let proj_dirs = ProjectDirs::from("", "", "karva").expect("Failed to get project directories");
    let cache_dir = proj_dirs.cache_dir();
    let test_cache = cache_dir.join("test-cache");
    Utf8PathBuf::from_path_buf(test_cache).expect("Path is not valid UTF-8")
}

/// Sets up the venv for tests.
///
/// On Unix: Creates a shared venv once and symlinks it into each test directory.
/// On Windows: Creates a fresh venv in each test directory (symlinks don't work with venvs).
#[cfg(unix)]
fn setup_venv(
    cache_dir: &Utf8Path,
    project_path: &Utf8Path,
    python_version: &str,
    karva_wheel_path: &str,
) -> Utf8PathBuf {
    let shared_venv = get_or_create_shared_venv(cache_dir, python_version, karva_wheel_path);
    let venv_link = project_path.join(".venv");

    std::os::unix::fs::symlink(shared_venv.as_std_path(), venv_link.as_std_path())
        .expect("Failed to symlink shared venv");

    shared_venv.clone()
}

/// Sets up the venv for tests.
///
/// On Windows: Creates a fresh venv in each test directory.
/// This is slower but necessary because Windows venv scripts have hardcoded paths
/// that break when accessed via symlinks.
#[cfg(windows)]
fn setup_venv(
    _cache_dir: &Utf8Path,
    project_path: &Utf8Path,
    python_version: &str,
    karva_wheel_path: &str,
) -> Utf8PathBuf {
    let venv_path = project_path.join(".venv");

    create_and_populate_venv(&venv_path, python_version, karva_wheel_path)
        .expect("Failed to create and populate test venv");

    venv_path
}

/// Returns a reference to the shared venv path, creating it if necessary.
///
/// The shared venv is stored in the cache directory and reused across all tests.
/// Uses file locking to coordinate venv creation across parallel test processes.
#[cfg(unix)]
fn get_or_create_shared_venv(
    cache_dir: &Utf8Path,
    python_version: &str,
    karva_wheel_path: &str,
) -> &'static Utf8PathBuf {
    SHARED_VENV.get_or_init(|| {
        let start = Instant::now();

        // Include wheel modification time in the venv name to invalidate when wheel changes
        let wheel_mtime = std::fs::metadata(karva_wheel_path)
            .and_then(|m| m.modified())
            .map(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            })
            .unwrap_or(0);

        let venv_name = format!("shared-venv-py{python_version}-{wheel_mtime}");
        let venv_path = cache_dir.join(&venv_name);

        // Use a lock file to coordinate venv creation across parallel test processes
        let lock_path = cache_dir.join(format!("{venv_name}.lock"));
        let lock_file = File::create(&lock_path).expect("Failed to create lock file");

        // Acquire exclusive lock (blocks until available)
        lock_file
            .lock_exclusive()
            .expect("Failed to acquire lock on venv lock file");

        // Check if the shared venv already exists and is valid (after acquiring lock)
        let venv_python = venv_path.join("bin").join("python");

        if !venv_python.exists() {
            // Clean up any partial/stale shared venvs
            if venv_path.exists() {
                let _ = std::fs::remove_dir_all(&venv_path);
            }

            // Clean up old shared venvs (from previous wheel builds)
            cleanup_old_shared_venvs(cache_dir, &venv_name);

            create_and_populate_venv(&venv_path, python_version, karva_wheel_path)
                .expect("Failed to create shared venv");

            eprintln!(
                "Created shared venv at {venv_path} in {:?}",
                start.elapsed()
            );
        } else {
            eprintln!("Reusing shared venv at {venv_path}");
        }

        // Lock is automatically released when lock_file goes out of scope
        drop(lock_file);

        venv_path
    })
}

/// Removes old shared venvs that are no longer needed.
#[cfg(unix)]
fn cleanup_old_shared_venvs(cache_dir: &Utf8Path, current_venv_name: &str) {
    let Ok(entries) = std::fs::read_dir(cache_dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        // Remove old shared venvs and their lock files (but not the current one)
        let is_lock_file = std::path::Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("lock"));

        if name.starts_with("shared-venv-") && !is_lock_file && name != current_venv_name {
            let _ = std::fs::remove_dir_all(&path);
            eprintln!("Cleaned up old shared venv: {name}");
        } else if name.starts_with("shared-venv-") && is_lock_file {
            let venv_name = name.trim_end_matches(".lock");
            if venv_name != current_venv_name {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

fn create_and_populate_venv(
    venv_path: &Utf8PathBuf,
    python_version: &str,
    karva_wheel_path: &str,
) -> anyhow::Result<()> {
    // 1. Create the venv with uv venv
    let status = Command::new("uv")
        .args(["venv", venv_path.as_str(), "--python", python_version])
        .stderr(Stdio::inherit()) // Show errors directly
        .status()
        .context("Failed to execute `uv venv`")?;

    if !status.success() {
        anyhow::bail!("`uv venv` failed with exit code {status}");
    }

    // 2. Install karva wheel + pytest (or any fixed baseline deps)
    let status = Command::new("uv")
        .args([
            "pip",
            "install",
            "--python",
            venv_path.as_str(),
            karva_wheel_path,
            "pytest==9.0.2", // or whatever fixed version you need
        ])
        .stderr(Stdio::inherit())
        .status()
        .context("Failed to install base packages into venv")?;

    if !status.success() {
        anyhow::bail!("Package installation failed with exit code {status}");
    }

    Ok(())
}
