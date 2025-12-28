use camino::{Utf8Component, Utf8Path, Utf8PathBuf};

pub fn absolute(path: impl AsRef<Utf8Path>, cwd: impl AsRef<Utf8Path>) -> Utf8PathBuf {
    let path = path.as_ref();
    let cwd = cwd.as_ref();

    let mut components = path.components().peekable();
    let mut ret = if let Some(c @ (Utf8Component::Prefix(..) | Utf8Component::RootDir)) =
        components.peek().copied()
    {
        components.next();
        Utf8PathBuf::from(c.as_str())
    } else {
        cwd.to_path_buf()
    };

    for component in components {
        match component {
            Utf8Component::Prefix(..) => unreachable!(),
            Utf8Component::RootDir => {
                ret.push(component.as_str());
            }
            Utf8Component::CurDir => {}
            Utf8Component::ParentDir => {
                ret.pop();
            }
            Utf8Component::Normal(c) => {
                ret.push(c);
            }
        }
    }

    ret
}
