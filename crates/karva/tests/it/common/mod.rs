use std::process::Command;

use anyhow::Context;
use camino::{Utf8Path, Utf8PathBuf};
use directories::ProjectDirs;
use insta::Settings;
use insta::internals::SettingsBindDropGuard;
use tempfile::TempDir;

pub struct TestContext {
    _temp_dir: TempDir,
    project_dir_path: Utf8PathBuf,
    _settings_scope: SettingsBindDropGuard,
}

// Use user cache directory so we can use `uv` caching.
pub fn get_test_cache_dir() -> Utf8PathBuf {
    let proj_dirs = ProjectDirs::from("", "", "karva").expect("Failed to get project directories");
    let cache_dir = proj_dirs.cache_dir();
    let test_cache = cache_dir.join("test-cache");
    Utf8PathBuf::from_path_buf(test_cache).expect("Path is not valid UTF-8")
}

impl TestContext {
    pub fn new() -> Self {
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

        // Create venv in project directory
        let venv_path = project_path.join(".venv");

        let karva_wheel = karva_system::find_karva_wheel()
            .expect(
                "Could not find karva wheel.

                Run `maturin build` before running tests.",
            )
            .to_string();

        let mut venv_args = vec!["venv", venv_path.as_str()];

        let env_python_version = std::env::var("PYTHON_VERSION");

        venv_args.push("-p");
        if let Ok(version) = &env_python_version {
            venv_args.push(version);
        } else {
            venv_args.push("3.13");
        }

        let command_arguments = [
            venv_args,
            vec![
                "pip",
                "install",
                "--python",
                venv_path.as_str(),
                &karva_wheel,
                "pytest==9.0.2",
            ],
        ];

        for arguments in &command_arguments {
            Command::new("uv")
                .args(arguments)
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

    fn venv_binary(&self, binary: &str) -> Utf8PathBuf {
        self.project_dir_path
            .join(".venv")
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
