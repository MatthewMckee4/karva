use crate::path::SystemPathBuf;

pub fn is_python_file(path: &SystemPathBuf) -> bool {
    path.extension() == Some("py")
}
