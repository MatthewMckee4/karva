pub mod path;

use anyhow::Context;
use camino::{Utf8Path, Utf8PathBuf};
use karva_metadata::{ProjectMetadata, ProjectSettings};

use crate::path::{TestPath, TestPathError, absolute};

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

#[derive(Debug, Clone)]
pub struct Project {
    settings: ProjectSettings,

    metadata: ProjectMetadata,
}

impl Project {
    pub fn from_metadata(metadata: ProjectMetadata) -> Self {
        let settings = metadata.options.to_settings();
        Self { settings, metadata }
    }

    pub const fn settings(&self) -> &ProjectSettings {
        &self.settings
    }

    pub const fn cwd(&self) -> &Utf8PathBuf {
        self.metadata.root()
    }

    pub fn test_paths(&self) -> Vec<Result<TestPath, TestPathError>> {
        let mut discovered_paths: Vec<Utf8PathBuf> = self
            .settings
            .src()
            .include_paths
            .iter()
            .map(|p| absolute(p, self.cwd()))
            .collect();

        if discovered_paths.is_empty() {
            discovered_paths.push(self.cwd().clone());
        }

        let test_paths: Vec<Result<TestPath, TestPathError>> = discovered_paths
            .iter()
            .map(|p| TestPath::new(p.as_str()))
            .collect();

        test_paths
    }

    pub const fn metadata(&self) -> &ProjectMetadata {
        &self.metadata
    }
}
