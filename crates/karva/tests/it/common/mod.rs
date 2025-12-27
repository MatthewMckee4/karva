use std::process::Command;

use anyhow::Context;
use camino::{Utf8Path, Utf8PathBuf};
use directories::ProjectDirs;
use insta::{Settings, internals::SettingsBindDropGuard};
use tempfile::TempDir;

pub struct TestContext {
    _temp_dir: TempDir,
    project_dir_path: Utf8PathBuf,
    _settings_scope: SettingsBindDropGuard,
}

// Use user cache directory so we can use `uv` caching.
fn get_test_venv_cache() -> Utf8PathBuf {
    let proj_dirs = ProjectDirs::from("", "", "karva").expect("Failed to get project directories");
    let cache_dir = proj_dirs.cache_dir();
    let venv_path = cache_dir.join("test-venv");
    Utf8PathBuf::from_path_buf(venv_path).expect("Path is not valid UTF-8")
}

impl TestContext {
    pub fn new() -> Self {
        let cache_dir = get_test_venv_cache();

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

        let karva_wheel = find_karva_wheel()
            .expect(
                "Could not find karva wheel.

                Run `maturin build` before running tests.",
            )
            .to_string();

        let venv_path = project_path.join(".venv");

        let mut venv_args = vec!["venv", venv_path.as_str()];

        let env_python_version = std::env::var("PYTHON_VERSION");

        venv_args.push("-p");
        if let Ok(version) = &env_python_version {
            venv_args.push(version);
        } else {
            venv_args.push("3.13");
        }

        let command_arguments = [
            // vec!["init", "--bare", "--directory", project_path.as_str()],
            venv_args,
            vec![
                "pip",
                "install",
                "--python",
                venv_path.as_str(),
                &karva_wheel,
                "pytest",
            ],
        ];

        for arguments in &command_arguments {
            Command::new("uv")
                .args(arguments)
                .current_dir(&project_path)
                .output()
                .with_context(|| format!("Failed to run command: {arguments:?}"))
                .unwrap();
        }

        let mut settings = Settings::clone_current();

        settings.add_filter(&tempdir_filter(&project_path), "<temp_dir>/");
        settings.add_filter(r#"\\(\w\w|\s|\.|")"#, "/$1");
        settings.add_filter(r"\x1b\[[0-9;]*m", "");
        settings.add_filter(r"(\s|\()(\d+m )?(\d+\.)?\d+(ms|s)", "$1[TIME]");

        let settings_scope = settings.bind_to_scope();

        Self {
            project_dir_path: project_path,
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

    pub fn karva_bin(&self) -> Utf8PathBuf {
        let venv_bin =
            self.root()
                .join(".venv")
                .join(if cfg!(windows) { "Scripts" } else { "bin" });
        venv_bin.join(if cfg!(windows) { "karva.exe" } else { "karva" })
    }
    pub fn command(&self) -> Command {
        let mut command = Command::new(self.karva_bin());
        command.current_dir(self.root()).arg("test");
        command
    }
}

impl Default for TestContext {
    fn default() -> Self {
        Self::new()
    }
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

pub fn tempdir_filter(path: &Utf8Path) -> String {
    format!(r"{}\\?/?", regex::escape(path.as_str()))
}
