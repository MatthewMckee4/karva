use crate::path::SystemPathBuf;

pub fn is_python_file(path: &SystemPathBuf) -> bool {
    path.extension() == Some("py")
}

pub fn module_name(cwd: &SystemPathBuf, path: &SystemPathBuf) -> String {
    let relative_path = path.strip_prefix(cwd).unwrap();
    relative_path.to_string()
}
