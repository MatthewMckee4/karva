use anyhow::Context;
use camino::{Utf8Path, Utf8PathBuf};

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
