use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use karva_static::EnvVars;

const KARVA_WORKER_BINARY_NAME: &str = "karva-worker";

/// Find the `karva-worker` binary by checking PATH, the project venv, and the active venv.
pub fn find_karva_worker_binary(current_dir: &Utf8Path) -> Result<Utf8PathBuf> {
    binary_from_path(KARVA_WORKER_BINARY_NAME)
        .or_else(|| venv_binary(KARVA_WORKER_BINARY_NAME, current_dir))
        .or_else(|| venv_binary_from_active_env(KARVA_WORKER_BINARY_NAME))
        .with_context(|| {
            format!(
                "Could not find {KARVA_WORKER_BINARY_NAME} binary in PATH, project .venv, or VIRTUAL_ENV"
            )
        })
}

fn binary_from_path(binary_name: &str) -> Option<Utf8PathBuf> {
    match which::which(binary_name) {
        Ok(path) => match Utf8PathBuf::try_from(path) {
            Ok(path) => {
                tracing::debug!(path = %path, "Found binary in PATH");
                Some(path)
            }
            Err(path) => {
                tracing::warn!(
                    path = ?path,
                    "Found binary in PATH, but its path is not valid UTF-8"
                );
                None
            }
        },
        Err(_) => None,
    }
}

/// Construct a platform-specific binary path within a virtual environment root directory.
fn construct_binary_path(venv_root: &Utf8Path, binary_name: &str) -> Utf8PathBuf {
    if cfg!(target_os = "windows") {
        venv_root.join("Scripts").join(format!("{binary_name}.exe"))
    } else {
        venv_root.join("bin").join(binary_name)
    }
}

/// Check if a binary exists within a virtual environment root and return its path.
fn venv_binary_at(venv_root: &Utf8Path, binary_name: &str) -> Option<Utf8PathBuf> {
    let binary_path = construct_binary_path(venv_root, binary_name);
    match binary_path.try_exists() {
        Ok(true) => Some(binary_path),
        Ok(false) => None,
        Err(err) => {
            tracing::warn!(path = %binary_path, "Failed to inspect virtualenv binary: {err}");
            None
        }
    }
}

fn venv_binary(binary_name: &str, directory: &Utf8Path) -> Option<Utf8PathBuf> {
    venv_binary_at(&directory.join(".venv"), binary_name)
}

fn venv_binary_from_active_env(binary_name: &str) -> Option<Utf8PathBuf> {
    let venv_root = std::env::var_os(EnvVars::VIRTUAL_ENV)?;
    let path = std::path::PathBuf::from(venv_root);
    let venv_root = match Utf8PathBuf::from_path_buf(path) {
        Ok(path) => path,
        Err(path) => {
            tracing::warn!(
                path = ?path,
                "Skipping active virtualenv because its path is not valid UTF-8"
            );
            return None;
        }
    };

    venv_binary_at(&venv_root, binary_name)
}

#[cfg(test)]
mod tests {
    use camino::Utf8Path;

    use super::{construct_binary_path, venv_binary_at};

    #[test]
    fn virtualenv_binary_uses_platform_script_directory() {
        let venv = Utf8Path::new("/tmp/project/.venv");

        let binary = construct_binary_path(venv, "karva-worker");

        if cfg!(target_os = "windows") {
            assert_eq!(binary, venv.join("Scripts").join("karva-worker.exe"));
        } else {
            assert_eq!(binary, venv.join("bin").join("karva-worker"));
        }
    }

    #[test]
    fn virtualenv_binary_requires_existing_candidate() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let venv = Utf8Path::from_path(tempdir.path()).expect("utf8 tempdir");

        assert_eq!(venv_binary_at(venv, "karva-worker"), None);

        let binary = construct_binary_path(venv, "karva-worker");
        std::fs::create_dir_all(binary.parent().expect("binary parent")).expect("mkdir");
        std::fs::write(&binary, "").expect("write binary");

        assert_eq!(venv_binary_at(venv, "karva-worker"), Some(binary));
    }
}
