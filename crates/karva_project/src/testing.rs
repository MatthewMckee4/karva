use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::Context;
use insta::{Settings, internals::SettingsBindDropGuard};
use tempfile::TempDir;

use crate::project::Project;

/// Find the karva wheel in the target/wheels directory.
/// Returns the path to the wheel file.
pub fn find_karva_wheel() -> anyhow::Result<PathBuf> {
    let karva_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .ok_or_else(|| anyhow::anyhow!("Could not determine KARVA_ROOT"))?
        .to_path_buf();

    let wheels_dir = karva_root.join("target").join("wheels");

    let entries = std::fs::read_dir(&wheels_dir)
        .with_context(|| format!("Could not read wheels directory: {}", wheels_dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let file_name = entry.file_name();
        if let Some(name) = file_name.to_str() {
            if name.starts_with("karva-")
                && std::path::Path::new(name)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("whl"))
            {
                return Ok(entry.path());
            }
        }
    }

    anyhow::bail!("Could not find karva wheel in target/wheels directory");
}

pub struct TestEnv {
    _temp_dir: TempDir,
    project_dir_path: PathBuf,
    mapped_paths: HashMap<String, PathBuf>,

    _settings_scope: SettingsBindDropGuard,
}

impl TestEnv {
    #[must_use]
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

        let settings_scope = settings.bind_to_scope();

        Self {
            project_dir_path: project_path,
            _temp_dir: temp_dir,
            mapped_paths,
            _settings_scope: settings_scope,
        }
    }

    #[must_use]
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

    #[must_use]
    pub fn temp_path(&self, path: impl AsRef<std::path::Path>) -> PathBuf {
        self.project_dir_path.join(path)
    }

    #[must_use]
    pub fn cwd(&self) -> PathBuf {
        self.project_dir_path.clone()
    }

    pub fn with_files<'a>(files: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut case = Self::new();
        case.write_files(files).unwrap();
        case
    }

    pub fn with_file(path: impl AsRef<Path>, content: &str) -> Self {
        let mut case = Self::new();
        case.write_file(path, content).unwrap();
        case
    }

    pub fn write_files<'a>(
        &mut self,
        files: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> anyhow::Result<()> {
        for (path, content) in files {
            self.write_file(path, content)?;
        }

        Ok(())
    }

    pub fn write_file(&mut self, path: impl AsRef<Path>, content: &str) -> anyhow::Result<()> {
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
                .with_context(|| format!("Failed to create directory `{}`", parent.display()))?;
        }
        std::fs::write(&path, &*ruff_python_trivia::textwrap::dedent(content))
            .with_context(|| format!("Failed to write file `{path}`", path = path.display()))?;

        Ok(())
    }

    #[must_use]
    pub fn mapped_path(&self, path: &str) -> Option<&PathBuf> {
        self.mapped_paths.get(path)
    }

    #[must_use]
    pub fn relative_path(&self, path: &Path) -> PathBuf {
        PathBuf::from(path.strip_prefix(self.cwd()).unwrap())
    }

    #[must_use]
    pub fn project(&self) -> Project {
        Project::new(self.cwd(), vec![self.cwd()])
    }
}

impl Default for TestEnv {
    fn default() -> Self {
        Self::new()
    }
}

fn tempdir_filter(path: &Path) -> String {
    format!(r"{}\\?/?", regex::escape(path.to_str().unwrap()))
}
