use std::io;

use camino::{Utf8Path, Utf8PathBuf};

use crate::format::SnapshotFile;

/// Return the snapshots directory for a given test file.
///
/// For a test file at `tests/test_example.py`, this returns `tests/snapshots/`.
pub fn snapshot_dir(test_file: &Utf8Path) -> Utf8PathBuf {
    if let Some(parent) = test_file.parent() {
        parent.join("snapshots")
    } else {
        Utf8PathBuf::from("snapshots")
    }
}

/// Return the path to a snapshot file.
///
/// Format: `{test_dir}/snapshots/{module_name}__{snapshot_name}.snap`
pub fn snapshot_path(test_file: &Utf8Path, module_name: &str, snapshot_name: &str) -> Utf8PathBuf {
    let dir = snapshot_dir(test_file);
    dir.join(format!("{module_name}__{snapshot_name}.snap"))
}

/// Return the path to a pending snapshot file (`.snap.new`).
pub fn pending_path(snap_path: &Utf8Path) -> Utf8PathBuf {
    Utf8PathBuf::from(format!("{snap_path}.new"))
}

/// Read and parse a snapshot file, returning `None` if it doesn't exist or can't be parsed.
pub fn read_snapshot(path: &Utf8Path) -> Option<SnapshotFile> {
    let content = std::fs::read_to_string(path).ok()?;
    SnapshotFile::parse(&content)
}

/// Write a snapshot file, creating parent directories as needed.
pub fn write_snapshot(path: &Utf8Path, snapshot: &SnapshotFile) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, snapshot.serialize())
}

/// Write a pending snapshot file (`.snap.new`), creating parent directories as needed.
pub fn write_pending_snapshot(snap_path: &Utf8Path, snapshot: &SnapshotFile) -> io::Result<()> {
    let pending = pending_path(snap_path);
    if let Some(parent) = pending.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(pending, snapshot.serialize())
}

/// Information about a pending snapshot found on disk.
#[derive(Debug, Clone)]
pub struct PendingSnapshotInfo {
    /// Path to the `.snap.new` file.
    pub pending_path: Utf8PathBuf,
    /// Path to the corresponding `.snap` file (may not exist yet).
    pub snap_path: Utf8PathBuf,
}

/// Recursively find all pending snapshot files (`.snap.new`) under a root directory.
pub fn find_pending_snapshots(root: &Utf8Path) -> Vec<PendingSnapshotInfo> {
    let mut results = Vec::new();
    find_pending_recursive(root, &mut results);
    results.sort_by(|a, b| a.pending_path.cmp(&b.pending_path));
    results
}

fn find_pending_recursive(dir: &Utf8Path, results: &mut Vec<PendingSnapshotInfo>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            if let Ok(utf8_path) = Utf8PathBuf::try_from(path) {
                find_pending_recursive(&utf8_path, results);
            }
        } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.ends_with(".snap.new") {
                if let Ok(pending_path) = Utf8PathBuf::try_from(path) {
                    let snap_path =
                        Utf8PathBuf::from(pending_path.as_str().strip_suffix(".new").unwrap_or(""));
                    results.push(PendingSnapshotInfo {
                        pending_path,
                        snap_path,
                    });
                }
            }
        }
    }
}

/// Accept a pending snapshot.
///
/// For inline snapshots (with `inline_source`/`inline_line` metadata),
/// rewrites the source file in-place and deletes the `.snap.new` file.
/// For file-based snapshots, renames `.snap.new` to `.snap`.
pub fn accept_pending(pending_path: &Utf8Path) -> io::Result<()> {
    if let Some(snapshot) = read_snapshot(pending_path) {
        if let (Some(source_file), Some(line)) = (
            &snapshot.metadata.inline_source,
            snapshot.metadata.inline_line,
        ) {
            let content = snapshot.content.trim_end();
            crate::inline::rewrite_inline_snapshot(source_file, line, content)?;
            return std::fs::remove_file(pending_path);
        }
    }

    let snap_path = pending_path
        .as_str()
        .strip_suffix(".new")
        .map(Utf8PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Not a .snap.new file"))?;
    std::fs::rename(pending_path, snap_path)
}

/// Reject a pending snapshot by deleting the `.snap.new` file.
pub fn reject_pending(pending_path: &Utf8Path) -> io::Result<()> {
    std::fs::remove_file(pending_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_dir() {
        let test_file = Utf8Path::new("tests/test_example.py");
        assert_eq!(
            snapshot_dir(test_file),
            Utf8PathBuf::from("tests/snapshots")
        );
    }

    #[test]
    fn test_snapshot_path() {
        let test_file = Utf8Path::new("tests/test_example.py");
        let path = snapshot_path(test_file, "test_example", "test_foo");
        assert_eq!(
            path,
            Utf8PathBuf::from("tests/snapshots/test_example__test_foo.snap")
        );
    }

    #[test]
    fn test_pending_path() {
        let snap = Utf8Path::new("tests/snapshots/test_example__test_foo.snap");
        assert_eq!(
            pending_path(snap),
            Utf8PathBuf::from("tests/snapshots/test_example__test_foo.snap.new")
        );
    }

    #[test]
    fn test_write_and_read_snapshot() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dir_path = Utf8Path::from_path(dir.path()).expect("utf8");
        let snap_path = dir_path.join("snapshots").join("mod__test.snap");

        let snapshot = SnapshotFile {
            metadata: crate::format::SnapshotMetadata {
                source: Some("test.py:3::test_foo".to_string()),
                ..Default::default()
            },
            content: "hello world\n".to_string(),
        };

        write_snapshot(&snap_path, &snapshot).expect("write");
        let read_back = read_snapshot(&snap_path).expect("read");
        assert_eq!(read_back, snapshot);
    }

    #[test]
    fn test_accept_pending() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dir_path = Utf8Path::from_path(dir.path()).expect("utf8");
        let snap_path = dir_path.join("test.snap");
        let pending = pending_path(&snap_path);

        std::fs::write(&pending, "content").expect("write pending");
        assert!(pending.exists());
        assert!(!snap_path.exists());

        accept_pending(&pending).expect("accept");
        assert!(!pending.exists());
        assert!(snap_path.exists());
    }

    #[test]
    fn test_reject_pending() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dir_path = Utf8Path::from_path(dir.path()).expect("utf8");
        let pending = dir_path.join("test.snap.new");

        std::fs::write(&pending, "content").expect("write pending");
        assert!(pending.exists());

        reject_pending(&pending).expect("reject");
        assert!(!pending.exists());
    }

    #[test]
    fn test_find_pending_snapshots() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dir_path = Utf8Path::from_path(dir.path()).expect("utf8");
        let snap_dir = dir_path.join("snapshots");
        std::fs::create_dir_all(&snap_dir).expect("mkdir");

        std::fs::write(snap_dir.join("mod__test1.snap.new"), "a").expect("write");
        std::fs::write(snap_dir.join("mod__test2.snap.new"), "b").expect("write");
        std::fs::write(snap_dir.join("mod__test3.snap"), "c").expect("write");

        let pending = find_pending_snapshots(dir_path);
        assert_eq!(pending.len(), 2);
    }
}
