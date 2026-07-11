pub mod path;

use anyhow::Context;
use camino::{Utf8Path, Utf8PathBuf};
use fs_err as fs;
use karva_metadata::{ProjectMetadata, ProjectSettings};

use crate::path::{TestPath, TestPathError, absolute};

/// Find the karva wheel in the target/wheels directory.
///
/// If multiple wheels are present (e.g. from previous builds), the most
/// recently modified one is returned so stale wheels don't interfere.
pub fn find_karva_wheel() -> anyhow::Result<Utf8PathBuf> {
    let karva_root = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .ok_or_else(|| anyhow::anyhow!("Could not determine KARVA_ROOT"))?
        .to_path_buf();

    let wheels_dir = karva_root.join("target").join("wheels");

    find_karva_wheel_in(&wheels_dir)
}

fn find_karva_wheel_in(wheels_dir: &Utf8Path) -> anyhow::Result<Utf8PathBuf> {
    let entries = fs::read_dir(wheels_dir)
        .with_context(|| format!("Could not read wheels directory: {wheels_dir}"))?;

    let mut newest: Option<(std::time::SystemTime, Utf8PathBuf)> = None;

    for entry in entries {
        let entry = entry
            .with_context(|| format!("Could not read entry in wheels directory: {wheels_dir}"))?;
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if !is_karva_wheel_name(name) {
            continue;
        }

        let path = wheels_dir.join(name);
        let mtime = entry
            .metadata()
            .with_context(|| format!("Could not read wheel metadata: {path}"))?
            .modified()
            .with_context(|| format!("Could not read wheel modification time: {path}"))?;

        if newest.as_ref().is_none_or(|(t, _)| mtime > *t) {
            newest = Some((mtime, path));
        }
    }

    newest
        .map(|(_, p)| p)
        .ok_or_else(|| anyhow::anyhow!("Could not find karva wheel in {wheels_dir}"))
}

fn is_karva_wheel_name(name: &str) -> bool {
    name.starts_with("karva-")
        && Utf8Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("whl"))
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

    pub fn settings(&self) -> &ProjectSettings {
        &self.settings
    }

    pub fn cwd(&self) -> &Utf8PathBuf {
        self.metadata.root()
    }

    pub fn test_paths(&self) -> Vec<Result<TestPath, TestPathError>> {
        if self.settings.src().include_paths.is_empty() {
            return vec![TestPath::new(self.cwd().as_str())];
        }

        self.settings
            .src()
            .include_paths
            .iter()
            .map(|path| {
                let path = absolute(path, self.cwd());
                TestPath::new(path.as_str())
            })
            .collect()
    }

    pub fn metadata(&self) -> &ProjectMetadata {
        &self.metadata
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{File, FileTimes};
    use std::time::{Duration, UNIX_EPOCH};

    use camino::Utf8Path;

    use super::find_karva_wheel_in;

    fn temp_path(dir: &tempfile::TempDir) -> &Utf8Path {
        Utf8Path::from_path(dir.path()).expect("temp path should be UTF-8")
    }

    fn write_wheel(root: &Utf8Path, name: &str, modified_after_epoch: u64) -> camino::Utf8PathBuf {
        let path = root.join(name);
        let file = File::create(&path).expect("create wheel");
        file.set_times(
            FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(modified_after_epoch)),
        )
        .expect("set wheel modified time");
        path
    }

    #[test]
    fn find_karva_wheel_returns_newest_matching_wheel() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let root = temp_path(&temp_dir);
        write_wheel(root, "karva-0.1.0-py3-none-any.whl", 10);
        write_wheel(root, "not-karva-9.9.9-py3-none-any.whl", 30);
        let newest = write_wheel(root, "karva-0.2.0-py3-none-any.WHL", 20);

        let wheel = find_karva_wheel_in(root).expect("find wheel");

        assert_eq!(wheel, newest);
    }

    #[test]
    fn find_karva_wheel_reports_missing_matching_wheel() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let root = temp_path(&temp_dir);
        write_wheel(root, "karva-0.1.0.tar.gz", 10);

        let err = find_karva_wheel_in(root).expect_err("missing wheel should fail");

        assert!(
            err.to_string().contains("Could not find karva wheel"),
            "{err:?}"
        );
    }
}
