use std::cmp::Reverse;
use std::collections::HashMap;
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

/// Recursively walk a directory tree and collect files that match a filter.
///
/// For each non-directory entry whose filename (as UTF-8) passes `filter`,
/// the entry's `Utf8PathBuf` is passed to `map` which may produce a value
/// to collect.
fn find_recursive<T>(
    dir: &Utf8Path,
    filter: &impl Fn(&str) -> bool,
    map: &impl Fn(Utf8PathBuf) -> Option<T>,
    results: &mut Vec<T>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            if let Ok(utf8_path) = Utf8PathBuf::try_from(path) {
                find_recursive(&utf8_path, filter, map, results);
            }
        } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if filter(name) {
                if let Ok(utf8_path) = Utf8PathBuf::try_from(path) {
                    if let Some(item) = map(utf8_path) {
                        results.push(item);
                    }
                }
            }
        }
    }
}

/// Recursively find all pending snapshot files (`.snap.new`) under a root directory.
pub fn find_pending_snapshots(root: &Utf8Path) -> Vec<PendingSnapshotInfo> {
    let mut results = Vec::new();
    find_recursive(
        root,
        &|name| name.ends_with(".snap.new"),
        &|pending_path| {
            let snap_path = Utf8PathBuf::from(pending_path.as_str().strip_suffix(".new")?);
            Some(PendingSnapshotInfo {
                pending_path,
                snap_path,
            })
        },
        &mut results,
    );
    results.sort_by(|a, b| a.pending_path.cmp(&b.pending_path));
    results
}

/// Extract the bare function name from a snapshot's `source` metadata.
///
/// Given a source like `test_file.py:5::TestClass::test_foo(x=1)`,
/// returns `Some("test_foo")`.
fn extract_function_name(source: Option<&str>) -> Option<&str> {
    source
        .and_then(|s| s.rsplit("::").next())
        .and_then(|s| s.split('(').next())
}

/// Accept a pending snapshot.
///
/// For inline snapshots (with `inline_source`/`inline_line` metadata),
/// rewrites the source file in-place and deletes the `.snap.new` file.
/// For file-based snapshots, renames `.snap.new` to `.snap`.
pub fn accept_pending(pending_path: &Utf8Path) -> io::Result<()> {
    if let Some(snapshot) = read_snapshot(pending_path)
        && let Some(source_file) = &snapshot.metadata.inline_source
        && let Some(line) = snapshot.metadata.inline_line
    {
        let content = snapshot.content.trim_end();
        let function_name = extract_function_name(snapshot.metadata.source.as_deref());
        crate::inline::rewrite_inline_snapshot(source_file, line, content, function_name)?;
        return std::fs::remove_file(pending_path);
    }

    let snap_path = pending_path
        .as_str()
        .strip_suffix(".new")
        .map(Utf8PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Not a .snap.new file"))?;
    std::fs::rename(pending_path, snap_path)
}

struct InlineInfo<'a> {
    pending_path: &'a Utf8Path,
    line: u32,
    content: String,
    function_name: Option<String>,
}

/// Classify pending snapshots into inline (grouped by source file) and file-based.
fn classify_pending_snapshots<'a>(
    pending: &'a [&PendingSnapshotInfo],
) -> (HashMap<String, Vec<InlineInfo<'a>>>, Vec<&'a Utf8Path>) {
    let mut inline_by_source: HashMap<String, Vec<InlineInfo<'_>>> = HashMap::new();
    let mut file_based: Vec<&Utf8Path> = Vec::new();

    for info in pending {
        if let Some(snapshot) = read_snapshot(&info.pending_path)
            && let Some(source_file) = snapshot.metadata.inline_source.clone()
            && let Some(line) = snapshot.metadata.inline_line
        {
            let function_name =
                extract_function_name(snapshot.metadata.source.as_deref()).map(String::from);
            inline_by_source
                .entry(source_file)
                .or_default()
                .push(InlineInfo {
                    pending_path: &info.pending_path,
                    line,
                    content: snapshot.content,
                    function_name,
                });
            continue;
        }
        file_based.push(&info.pending_path);
    }

    (inline_by_source, file_based)
}

/// Process inline snapshots in descending line order within each source file.
///
/// Processing bottom-to-top ensures that multiline expansions at higher lines
/// don't shift line numbers for edits above them.
fn process_inline_snapshots(
    inline_by_source: &mut HashMap<String, Vec<InlineInfo<'_>>>,
) -> io::Result<()> {
    for (source_file, group) in inline_by_source.iter_mut() {
        group.sort_by_key(|info| Reverse(info.line));
        for item in group.iter() {
            let content = item.content.trim_end();
            crate::inline::rewrite_inline_snapshot(
                source_file,
                item.line,
                content,
                item.function_name.as_deref(),
            )?;
            std::fs::remove_file(item.pending_path)?;
        }
    }
    Ok(())
}

/// Process file-based pending snapshots by renaming `.snap.new` to `.snap`.
fn process_file_based_snapshots(file_based: &[&Utf8Path]) -> io::Result<()> {
    for path in file_based {
        accept_pending(path)?;
    }
    Ok(())
}

/// Accept multiple pending snapshots, processing inline snapshots in reverse
/// line order within each source file.
///
/// When multiple inline snapshots target the same source file, each multiline
/// expansion shifts line numbers for subsequent snapshots. By processing in
/// descending line order (bottom-to-top), edits at higher lines don't affect
/// line numbers above.
pub fn accept_pending_batch(pending: &[&PendingSnapshotInfo]) -> io::Result<()> {
    let (mut inline_by_source, file_based) = classify_pending_snapshots(pending);
    process_inline_snapshots(&mut inline_by_source)?;
    process_file_based_snapshots(&file_based)
}

/// Reject a pending snapshot by deleting the `.snap.new` file.
pub fn reject_pending(pending_path: &Utf8Path) -> io::Result<()> {
    std::fs::remove_file(pending_path)
}

/// Information about a snapshot file found on disk.
#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    pub snap_path: Utf8PathBuf,
}

/// Why a snapshot is considered unreferenced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnreferencedReason {
    NoSource,
    TestFileNotFound(String),
    FunctionNotFound { file: String, function: String },
}

impl std::fmt::Display for UnreferencedReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSource => write!(f, "no source metadata"),
            Self::TestFileNotFound(file) => write!(f, "test file not found: {file}"),
            Self::FunctionNotFound { file, function } => {
                write!(f, "function `{function}` not found in {file}")
            }
        }
    }
}

/// A snapshot whose source test no longer exists.
#[derive(Debug, Clone)]
pub struct UnreferencedSnapshot {
    pub snap_path: Utf8PathBuf,
    pub reason: UnreferencedReason,
}

/// Parse a snapshot's `source` metadata field into `(filename, snapshot_name)`.
///
/// Handles formats like `test.py:5::test_name` and `test.py::test_name`.
pub fn parse_source(source: &str) -> Option<(&str, &str)> {
    let (file, name) = source.split_once("::")?;
    let file = file.rsplit_once(':').map_or(file, |(f, _)| f);
    if file.is_empty() || name.is_empty() {
        return None;
    }
    Some((file, name))
}

/// Strip suffixes from a snapshot name to get the base function name.
///
/// Strips parametrize params `test_foo(x=1)` → `test_foo`,
/// numbering `test_foo-2` → `test_foo`,
/// inline suffix `test_foo_inline_5` → `test_foo`,
/// and class prefix `TestClass::test_method` → `test_method`.
pub fn base_function_name(name: &str) -> &str {
    let name = name.rsplit_once("::").map_or(name, |(_, method)| method);
    let name = name.split_once("--").map_or(name, |(base, _)| base);
    let name = name.split_once('(').map_or(name, |(base, _)| base);
    let name = name.rsplit_once('-').map_or(name, |(base, suffix)| {
        if suffix.chars().all(|c| c.is_ascii_digit()) {
            base
        } else {
            name
        }
    });
    let digits_stripped = name.trim_end_matches(|c: char| c.is_ascii_digit());
    if digits_stripped.len() < name.len() {
        if let Some(base) = digits_stripped.strip_suffix("_inline_") {
            return base;
        }
    }
    name
}

/// Check whether a function definition `def {name}(` exists in a file.
pub fn function_exists_in_file(path: &Utf8Path, name: &str) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    let pattern = format!("def {name}(");
    content.contains(&pattern)
}

/// Recursively find all committed snapshot files (`.snap`, not `.snap.new`).
pub fn find_snapshots(root: &Utf8Path) -> Vec<SnapshotInfo> {
    let mut results = Vec::new();
    find_recursive(
        root,
        &|name| {
            std::path::Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("snap"))
                && !name.ends_with(".snap.new")
        },
        &|snap_path| Some(SnapshotInfo { snap_path }),
        &mut results,
    );
    results.sort_by(|a, b| a.snap_path.cmp(&b.snap_path));
    results
}

/// A snapshot file of any kind (`.snap` or `.snap.new`) found on disk.
#[derive(Debug, Clone)]
pub struct AnySnapshotInfo {
    pub path: Utf8PathBuf,
}

/// Recursively find all snapshot files (`.snap` and `.snap.new`) under a root directory.
pub fn find_all_snapshots(root: &Utf8Path) -> Vec<AnySnapshotInfo> {
    let mut results = Vec::new();
    find_recursive(
        root,
        &|name| {
            name.ends_with(".snap.new")
                || std::path::Path::new(name)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("snap"))
        },
        &|path| Some(AnySnapshotInfo { path }),
        &mut results,
    );
    results.sort_by(|a, b| a.path.cmp(&b.path));
    results
}

/// Find all snapshot files whose source test no longer exists.
pub fn find_unreferenced_snapshots(root: &Utf8Path) -> Vec<UnreferencedSnapshot> {
    let snapshots = find_snapshots(root);
    let mut unreferenced = Vec::new();

    for info in &snapshots {
        let reason = check_snapshot_reference(info);
        if let Some(reason) = reason {
            unreferenced.push(UnreferencedSnapshot {
                snap_path: info.snap_path.clone(),
                reason,
            });
        }
    }

    unreferenced
}

fn check_snapshot_reference(info: &SnapshotInfo) -> Option<UnreferencedReason> {
    let snapshot = read_snapshot(&info.snap_path)?;

    let Some(source) = &snapshot.metadata.source else {
        return Some(UnreferencedReason::NoSource);
    };

    let Some((file_name, snapshot_name)) = parse_source(source) else {
        return Some(UnreferencedReason::NoSource);
    };

    let snapshots_dir = info.snap_path.parent()?;
    let test_dir = snapshots_dir.parent()?;
    let test_file = test_dir.join(file_name);

    if !test_file.exists() {
        return Some(UnreferencedReason::TestFileNotFound(file_name.to_string()));
    }

    let func_name = base_function_name(snapshot_name);
    if !function_exists_in_file(&test_file, func_name) {
        return Some(UnreferencedReason::FunctionNotFound {
            file: file_name.to_string(),
            function: func_name.to_string(),
        });
    }

    None
}

/// Remove a snapshot file. Also removes the parent directory if it becomes empty.
pub fn remove_snapshot(path: &Utf8Path) -> io::Result<()> {
    std::fs::remove_file(path)?;
    if let Some(parent) = path.parent() {
        if parent.file_name().is_some_and(|name| name == "snapshots") {
            let _ = std::fs::remove_dir(parent);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Normalize path separators for cross-platform snapshot stability.
    fn normalize_path(path: &Utf8Path) -> String {
        path.as_str().replace('\\', "/")
    }

    #[test]
    fn snapshot_dir_for_test_file() {
        insta::assert_snapshot!(
            normalize_path(&snapshot_dir(Utf8Path::new("tests/test_example.py"))),
            @"tests/snapshots"
        );
    }

    #[test]
    fn snapshot_path_for_module_and_name() {
        insta::assert_snapshot!(
            normalize_path(&snapshot_path(Utf8Path::new("tests/test_example.py"), "test_example", "test_foo")),
            @"tests/snapshots/test_example__test_foo.snap"
        );
    }

    #[test]
    fn pending_path_appends_new() {
        insta::assert_snapshot!(
            normalize_path(&pending_path(Utf8Path::new("tests/snapshots/test_example__test_foo.snap"))),
            @"tests/snapshots/test_example__test_foo.snap.new"
        );
    }

    #[test]
    fn write_and_read_snapshot() {
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
    fn accept_pending_renames_file() {
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
    fn reject_pending_deletes_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dir_path = Utf8Path::from_path(dir.path()).expect("utf8");
        let pending = dir_path.join("test.snap.new");

        std::fs::write(&pending, "content").expect("write pending");
        assert!(pending.exists());

        reject_pending(&pending).expect("reject");
        assert!(!pending.exists());
    }

    #[test]
    fn find_pending_excludes_committed() {
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

    #[test]
    fn parse_source_with_line_number() {
        let (file, name) = parse_source("test.py:5::test_foo").expect("parse");
        insta::assert_snapshot!(file, @"test.py");
        insta::assert_snapshot!(name, @"test_foo");
    }

    #[test]
    fn parse_source_without_line_number() {
        let (file, name) = parse_source("test.py::test_foo").expect("parse");
        insta::assert_snapshot!(file, @"test.py");
        insta::assert_snapshot!(name, @"test_foo");
    }

    #[test]
    fn parse_source_parametrized() {
        let (file, name) = parse_source("test.py:6::test_param(x=1)").expect("parse");
        insta::assert_snapshot!(file, @"test.py");
        insta::assert_snapshot!(name, @"test_param(x=1)");
    }

    #[test]
    fn parse_source_invalid() {
        assert!(parse_source("no_separator").is_none());
        assert!(parse_source("::name_only").is_none());
        assert!(parse_source("file::").is_none());
    }

    #[test]
    fn base_function_name_simple() {
        insta::assert_snapshot!(base_function_name("test_foo"), @"test_foo");
    }

    #[test]
    fn base_function_name_parametrized() {
        insta::assert_snapshot!(base_function_name("test_foo(x=1)"), @"test_foo");
    }

    #[test]
    fn base_function_name_numbered() {
        insta::assert_snapshot!(base_function_name("test_foo-2"), @"test_foo");
        insta::assert_snapshot!(base_function_name("test_foo-13"), @"test_foo");
    }

    #[test]
    fn base_function_name_inline() {
        insta::assert_snapshot!(base_function_name("test_foo_inline_5"), @"test_foo");
    }

    #[test]
    fn base_function_name_inline_multi_digit() {
        insta::assert_snapshot!(base_function_name("test_foo_inline_15"), @"test_foo");
        insta::assert_snapshot!(base_function_name("test_foo_inline_123"), @"test_foo");
    }

    #[test]
    fn base_function_name_class_prefix() {
        insta::assert_snapshot!(base_function_name("TestClass::test_method"), @"test_method");
    }

    #[test]
    fn base_function_name_named() {
        insta::assert_snapshot!(base_function_name("test_foo--header"), @"test_foo");
        insta::assert_snapshot!(base_function_name("test_foo--header(x=1)"), @"test_foo");
    }

    #[test]
    fn find_snapshots_excludes_snap_new() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dir_path = Utf8Path::from_path(dir.path()).expect("utf8");
        let snap_dir = dir_path.join("snapshots");
        std::fs::create_dir_all(&snap_dir).expect("mkdir");

        std::fs::write(snap_dir.join("mod__test1.snap"), "a").expect("write");
        std::fs::write(snap_dir.join("mod__test2.snap.new"), "b").expect("write");
        std::fs::write(snap_dir.join("mod__test3.snap"), "c").expect("write");

        let snaps = find_snapshots(dir_path);
        assert_eq!(snaps.len(), 2);
    }

    #[test]
    fn unreferenced_file_not_found() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dir_path = Utf8Path::from_path(dir.path()).expect("utf8");
        let snap_dir = dir_path.join("snapshots");
        std::fs::create_dir_all(&snap_dir).expect("mkdir");

        let snapshot = SnapshotFile {
            metadata: crate::format::SnapshotMetadata {
                source: Some("test.py:5::test_foo".to_string()),
                ..Default::default()
            },
            content: "hello\n".to_string(),
        };
        write_snapshot(&snap_dir.join("test__test_foo.snap"), &snapshot).expect("write");

        let unreferenced = find_unreferenced_snapshots(dir_path);
        assert_eq!(unreferenced.len(), 1);
        insta::assert_snapshot!(unreferenced[0].reason, @"test file not found: test.py");
    }

    #[test]
    fn unreferenced_function_not_found() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dir_path = Utf8Path::from_path(dir.path()).expect("utf8");
        let snap_dir = dir_path.join("snapshots");
        std::fs::create_dir_all(&snap_dir).expect("mkdir");

        std::fs::write(dir_path.join("test.py"), "def test_other():\n    pass\n").expect("write");

        let snapshot = SnapshotFile {
            metadata: crate::format::SnapshotMetadata {
                source: Some("test.py:5::test_foo".to_string()),
                ..Default::default()
            },
            content: "hello\n".to_string(),
        };
        write_snapshot(&snap_dir.join("test__test_foo.snap"), &snapshot).expect("write");

        let unreferenced = find_unreferenced_snapshots(dir_path);
        assert_eq!(unreferenced.len(), 1);
        insta::assert_snapshot!(unreferenced[0].reason, @"function `test_foo` not found in test.py");
    }

    #[test]
    fn referenced_function_exists() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dir_path = Utf8Path::from_path(dir.path()).expect("utf8");
        let snap_dir = dir_path.join("snapshots");
        std::fs::create_dir_all(&snap_dir).expect("mkdir");

        std::fs::write(dir_path.join("test.py"), "def test_foo():\n    pass\n").expect("write");

        let snapshot = SnapshotFile {
            metadata: crate::format::SnapshotMetadata {
                source: Some("test.py:5::test_foo".to_string()),
                ..Default::default()
            },
            content: "hello\n".to_string(),
        };
        write_snapshot(&snap_dir.join("test__test_foo.snap"), &snapshot).expect("write");

        let unreferenced = find_unreferenced_snapshots(dir_path);
        assert!(unreferenced.is_empty());
    }

    #[test]
    fn remove_snapshot_cleans_empty_dir() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dir_path = Utf8Path::from_path(dir.path()).expect("utf8");
        let snap_dir = dir_path.join("snapshots");
        std::fs::create_dir_all(&snap_dir).expect("mkdir");

        let snap_path = snap_dir.join("test__test_foo.snap");
        std::fs::write(&snap_path, "content").expect("write");

        remove_snapshot(&snap_path).expect("remove");
        assert!(!snap_path.exists());
        assert!(!snap_dir.exists());
    }
}
