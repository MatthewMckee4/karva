use camino::Utf8PathBuf;

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
