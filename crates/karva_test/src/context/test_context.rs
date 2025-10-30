use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::Context;
use insta::{Settings, internals::SettingsBindDropGuard};
use tempfile::TempDir;

use crate::{find_karva_wheel, utils::tempdir_filter};

pub struct TestContext {
    _temp_dir: TempDir,
    project_dir_path: PathBuf,
    mapped_paths: HashMap<String, PathBuf>,
    _settings_scope: SettingsBindDropGuard,
}

impl TestContext {
    pub fn new() -> Self {
        let temp_dir = TempDir::with_prefix("karva-test-env").unwrap();

        let project_path = dunce::simplified(
            &temp_dir
                .path()
                .canonicalize()
                .context("Failed to canonicalize project path")
                .unwrap(),
        )
        .to_path_buf();

        let karva_wheel = find_karva_wheel().unwrap();

        let venv_path = project_path.join(".venv");

        let commands = [
            vec![
                "uv",
                "init",
                "--bare",
                "--directory",
                project_path.to_str().unwrap(),
            ],
            vec!["uv", "venv", venv_path.to_str().unwrap(), "-p", "3.13"],
            vec![
                "uv",
                "pip",
                "install",
                "--python",
                venv_path.to_str().unwrap(),
                karva_wheel.to_str().unwrap(),
                "pytest",
            ],
        ];

        for command in &commands {
            Command::new(command[0])
                .args(&command[1..])
                .current_dir(&project_path)
                .output()
                .with_context(|| format!("Failed to run command: {command:?}"))
                .unwrap();
        }

        let mut settings = Settings::clone_current();

        let mut mapped_paths = HashMap::new();
        for test_name in ["<test>".to_string(), "<test2>".to_string()] {
            let mapped_test_dir = format!("main_{}", rand::random::<u32>());

            let mapped_test_path = project_path.join(mapped_test_dir.clone());

            fs::create_dir_all(&mapped_test_path).unwrap();
            mapped_paths.insert(test_name.clone(), mapped_test_path);
            settings.add_filter(&mapped_test_dir, test_name);
        }

        settings.add_filter(&tempdir_filter(&project_path), "<temp_dir>/");
        settings.add_filter(r#"\\(\w\w|\s|\.|")"#, "/$1");
        settings.add_filter(r"(\s|\()(\d+m )?(\d+\.)?\d+(ms|s)", "$1[TIME]");

        let settings_scope = settings.bind_to_scope();

        Self {
            project_dir_path: project_path,
            _temp_dir: temp_dir,
            mapped_paths,
            _settings_scope: settings_scope,
        }
    }

    fn create_random_dir(&self) -> PathBuf {
        self.create_dir(format!("main_{}", rand::random::<u32>()))
    }

    pub fn create_file(&self, path: impl AsRef<std::path::Path>, content: &str) -> PathBuf {
        let path = path.as_ref();
        let path = self.project_dir_path.join(path);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, &*ruff_python_trivia::textwrap::dedent(content)).unwrap();

        path
    }

    #[allow(clippy::must_use_candidate)]
    pub fn create_dir(&self, path: impl AsRef<std::path::Path>) -> PathBuf {
        let path = self.project_dir_path.join(path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    pub fn temp_path(&self, path: impl AsRef<std::path::Path>) -> PathBuf {
        self.project_dir_path.join(path)
    }

    pub fn cwd(&self) -> PathBuf {
        self.project_dir_path.clone()
    }

    pub fn with_files<'a>(files: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut case = Self::default();
        case.write_files(files);
        case
    }

    pub fn with_file(path: impl AsRef<Path>, content: &str) -> Self {
        let mut case = Self::default();
        case.write_file(path, content);
        case
    }

    pub fn write_files<'a>(&mut self, files: impl IntoIterator<Item = (&'a str, &'a str)>) {
        for (path, content) in files {
            self.write_file(path, content);
        }
    }

    pub fn write_file(&mut self, path: impl AsRef<Path>, content: &str) {
        // If the path starts with "<test>/", we want to map "<test>" to a temp dir.
        let path = path.as_ref();
        let mut components = path.components();

        // Check if the first component is a normal component that looks like "<test>"
        let mut mapped_path = None;
        if let Some(std::path::Component::Normal(first)) = components.next() {
            if let Some(test_name) = first.to_str() {
                // Only map components that start and end with angle brackets
                if test_name.starts_with('<') && test_name.ends_with('>') {
                    let base_dir = if let Some(existing_path) = self.mapped_paths.get(test_name) {
                        existing_path.clone()
                    } else {
                        let new_path = self.create_random_dir();

                        self.mapped_paths
                            .insert(test_name.to_string(), new_path.clone());

                        new_path
                    };

                    let rest: std::path::PathBuf = components.collect();
                    mapped_path = Some(base_dir.join(rest));
                }
            }
        }
        let path = mapped_path.unwrap_or_else(|| self.project_dir_path.join(path));

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory `{}`", parent.display()))
                .unwrap();
        }
        std::fs::write(&path, &*ruff_python_trivia::textwrap::dedent(content))
            .with_context(|| format!("Failed to write file `{path}`", path = path.display()))
            .unwrap();
    }

    pub fn mapped_path(&self, path: &str) -> Option<&PathBuf> {
        self.mapped_paths.get(path)
    }
}

impl Default for TestContext {
    fn default() -> Self {
        Self::new()
    }
}
