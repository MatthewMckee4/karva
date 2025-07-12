pub mod test_path;

use std::path::{Component, Path, PathBuf};

pub use test_path::{TestPath, TestPathError};

pub fn absolute(path: impl AsRef<SystemPath>, cwd: impl AsRef<SystemPath>) -> SystemPathBuf {
    let path = path.as_ref();
    let cwd = cwd.as_ref();

    let mut components = path.components().peekable();
    let mut ret = if let Some(c @ (Component::Prefix(..) | Component::RootDir)) =
        components.peek().copied()
    {
        components.next();
        PathBuf::from(c.as_os_str())
    } else {
        cwd.to_path_buf()
    };

    for component in components {
        match component {
            Component::Prefix(..) => unreachable!(),
            Component::RootDir => {
                ret.push(component);
            }
            Component::CurDir => {}
            Component::ParentDir => {
                ret.pop();
            }
            Component::Normal(c) => {
                ret.push(c);
            }
        }
    }

    SystemPathBuf::from(ret)
}

pub type SystemPath = Path;

pub type SystemPathBuf = PathBuf;
