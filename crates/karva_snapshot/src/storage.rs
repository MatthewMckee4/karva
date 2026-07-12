use std::cmp::Reverse;
use std::collections::HashMap;
use std::io;

use camino::{Utf8Path, Utf8PathBuf};
use fs_err as fs;

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

/// Read and parse a snapshot file.
///
/// Returns `Ok(None)` when the file does not exist.
pub fn read_snapshot(path: &Utf8Path) -> io::Result<Option<SnapshotFile>> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };

    SnapshotFile::parse(&content).map(Some).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("malformed snapshot file `{path}`: {err}"),
        )
    })
}

/// Write a snapshot file, creating parent directories as needed.
pub fn write_snapshot(path: &Utf8Path, snapshot: &SnapshotFile) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, snapshot.serialize())
}

/// Write a pending snapshot file (`.snap.new`), creating parent directories as needed.
pub fn write_pending_snapshot(snap_path: &Utf8Path, snapshot: &SnapshotFile) -> io::Result<()> {
    let pending = pending_path(snap_path);
    if let Some(parent) = pending.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(pending, snapshot.serialize())
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
) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            if let Ok(utf8_path) = Utf8PathBuf::try_from(path) {
                find_recursive(&utf8_path, filter, map, results)?;
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

    Ok(())
}

/// Recursively find all pending snapshot files (`.snap.new`) under a root directory.
pub fn find_pending_snapshots(root: &Utf8Path) -> io::Result<Vec<PendingSnapshotInfo>> {
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
    )?;
    results.sort_by(|a, b| a.pending_path.cmp(&b.pending_path));
    Ok(results)
}

/// Check whether a generated snapshot path matches a user filter.
///
/// Filters can target either the generated snapshot file or the source file
/// whose stem is encoded in that snapshot filename.
pub fn matches_snapshot_filter(snapshot_path: &Utf8Path, filter: &Utf8Path) -> bool {
    matches_filter_path(snapshot_path, filter)
        || source_path_for_snapshot(snapshot_path)
            .is_some_and(|source| matches_filter_path(&source, filter))
}

fn matches_filter_path(path: &Utf8Path, filter: &Utf8Path) -> bool {
    path.starts_with(filter) || matches_snapshot_file_stem(path, filter)
}

fn matches_snapshot_file_stem(path: &Utf8Path, filter: &Utf8Path) -> bool {
    if path.parent() != filter.parent() {
        return false;
    }

    let Some(file_name) = path.file_name() else {
        return false;
    };
    let Some(filter_name) = filter.file_name() else {
        return false;
    };
    let Some(rest) = file_name.strip_prefix(filter_name) else {
        return false;
    };

    rest.starts_with("__") || rest.starts_with(".snap")
}

fn source_path_for_snapshot(snapshot_path: &Utf8Path) -> Option<Utf8PathBuf> {
    let snapshots_dir = snapshot_path.parent()?;
    if snapshots_dir.file_name()? != "snapshots" {
        return None;
    }

    let file_name = snapshot_path.file_name()?;
    let snapshot_stem = file_name
        .strip_suffix(".snap.new")
        .or_else(|| file_name.strip_suffix(".snap"))?;
    let (module_name, _) = snapshot_stem.split_once("__")?;
    let source_dir = snapshots_dir.parent()?;

    Some(source_dir.join(format!("{module_name}.py")))
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
    if let Some(snapshot) = read_snapshot(pending_path)?
        && let Some(source_file) = &snapshot.metadata.inline_source
        && let Some(line) = snapshot.metadata.inline_line
    {
        let content = snapshot.content.trim_end();
        let function_name = extract_function_name(snapshot.metadata.source.as_deref());
        crate::inline::rewrite_inline_snapshot(source_file, line, content, function_name)?;
        return fs::remove_file(pending_path);
    }

    let snap_path = pending_path
        .as_str()
        .strip_suffix(".new")
        .map(Utf8PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Not a .snap.new file"))?;
    fs::rename(pending_path, snap_path)
}

struct InlineInfo<'a> {
    pending_path: &'a Utf8Path,
    line: u32,
    content: String,
    function_name: Option<String>,
}

type ClassifiedPendingSnapshots<'a> = (HashMap<String, Vec<InlineInfo<'a>>>, Vec<&'a Utf8Path>);

/// Classify pending snapshots into inline (grouped by source file) and file-based.
fn classify_pending_snapshots(
    pending: &[PendingSnapshotInfo],
) -> io::Result<ClassifiedPendingSnapshots<'_>> {
    let mut inline_by_source: HashMap<String, Vec<InlineInfo<'_>>> = HashMap::new();
    let mut file_based: Vec<&Utf8Path> = Vec::new();

    for info in pending {
        if let Some(snapshot) = read_snapshot(&info.pending_path)?
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

    Ok((inline_by_source, file_based))
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
            fs::remove_file(item.pending_path)?;
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
pub fn accept_pending_batch(pending: &[PendingSnapshotInfo]) -> io::Result<()> {
    let (mut inline_by_source, file_based) = classify_pending_snapshots(pending)?;
    process_inline_snapshots(&mut inline_by_source)?;
    process_file_based_snapshots(&file_based)
}

/// Reject a pending snapshot by deleting the `.snap.new` file.
pub fn reject_pending(pending_path: &Utf8Path) -> io::Result<()> {
    fs::remove_file(pending_path)
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
    InvalidSource(String),
    TestFileNotFound(String),
    FunctionNotFound { file: String, function: String },
}

impl std::fmt::Display for UnreferencedReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSource => write!(f, "no source metadata"),
            Self::InvalidSource(source) => write!(f, "invalid source metadata: {source}"),
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
pub fn function_exists_in_file(path: &Utf8Path, name: &str) -> io::Result<bool> {
    let content = fs::read_to_string(path).map_err(|err| {
        io::Error::new(
            err.kind(),
            format!("failed to read source file `{path}`: {err}"),
        )
    })?;
    Ok(content
        .lines()
        .any(|line| line_declares_function(line, name)))
}

fn line_declares_function(line: &str, name: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed
        .strip_prefix("def ")
        .or_else(|| trimmed.strip_prefix("async def "))
    else {
        return false;
    };

    rest.strip_prefix(name)
        .is_some_and(|after_name| after_name.starts_with('('))
}

/// Recursively find all committed snapshot files (`.snap`, not `.snap.new`).
pub fn find_snapshots(root: &Utf8Path) -> io::Result<Vec<SnapshotInfo>> {
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
    )?;
    results.sort_by(|a, b| a.snap_path.cmp(&b.snap_path));
    Ok(results)
}

/// A snapshot file of any kind (`.snap` or `.snap.new`) found on disk.
#[derive(Debug, Clone)]
pub struct AnySnapshotInfo {
    pub path: Utf8PathBuf,
}

/// Recursively find all snapshot files (`.snap` and `.snap.new`) under a root directory.
pub fn find_all_snapshots(root: &Utf8Path) -> io::Result<Vec<AnySnapshotInfo>> {
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
    )?;
    results.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(results)
}

/// Find all snapshot files whose source test no longer exists.
pub fn find_unreferenced_snapshots(root: &Utf8Path) -> io::Result<Vec<UnreferencedSnapshot>> {
    let snapshots = find_snapshots(root)?;
    let mut unreferenced = Vec::new();

    for info in &snapshots {
        let reason = check_snapshot_reference(info)?;
        if let Some(reason) = reason {
            unreferenced.push(UnreferencedSnapshot {
                snap_path: info.snap_path.clone(),
                reason,
            });
        }
    }

    Ok(unreferenced)
}

fn check_snapshot_reference(info: &SnapshotInfo) -> io::Result<Option<UnreferencedReason>> {
    let Some(snapshot) = read_snapshot(&info.snap_path)? else {
        return Ok(None);
    };

    let Some(source) = &snapshot.metadata.source else {
        return Ok(Some(UnreferencedReason::NoSource));
    };

    let Some((file_name, snapshot_name)) = parse_source(source) else {
        return Ok(Some(UnreferencedReason::InvalidSource(source.clone())));
    };

    let Some(snapshots_dir) = info.snap_path.parent() else {
        return Ok(None);
    };
    let Some(test_dir) = snapshots_dir.parent() else {
        return Ok(None);
    };
    let test_file = test_dir.join(file_name);

    if !test_file.exists() {
        return Ok(Some(UnreferencedReason::TestFileNotFound(
            file_name.to_string(),
        )));
    }

    let func_name = base_function_name(snapshot_name);
    if !function_exists_in_file(&test_file, func_name)? {
        return Ok(Some(UnreferencedReason::FunctionNotFound {
            file: file_name.to_string(),
            function: func_name.to_string(),
        }));
    }

    Ok(None)
}

/// Remove a snapshot file. Also removes the parent directory if it becomes empty.
pub fn remove_snapshot(path: &Utf8Path) -> io::Result<()> {
    fs::remove_file(path)?;
    if let Some(parent) = path.parent() {
        if parent.file_name().is_some_and(|name| name == "snapshots") {
            match fs::remove_dir(parent) {
                Ok(()) => {}
                Err(err)
                    if matches!(
                        err.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
                    ) => {}
                Err(err) => return Err(err),
            }
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
        let read_back = read_snapshot(&snap_path).expect("read").expect("snapshot");
        assert_eq!(read_back, snapshot);
    }

    #[test]
    fn read_snapshot_reports_malformed_files() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dir_path = Utf8Path::from_path(dir.path()).expect("utf8");
        let snap_path = dir_path.join("snapshots").join("mod__test.snap");

        if let Some(parent) = snap_path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(&snap_path, "not a snapshot").expect("write");

        let err = read_snapshot(&snap_path).expect_err("malformed snapshot should fail");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(
            err.to_string()
                .contains("missing opening frontmatter separator"),
            "{err}"
        );
    }

    #[test]
    fn accept_pending_renames_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dir_path = Utf8Path::from_path(dir.path()).expect("utf8");
        let snap_path = dir_path.join("test.snap");
        let pending = pending_path(&snap_path);
        let snapshot = SnapshotFile {
            metadata: crate::format::SnapshotMetadata::default(),
            content: "content\n".to_string(),
        };

        write_pending_snapshot(&snap_path, &snapshot).expect("write pending");
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
    fn reject_pending_missing_file_reports_path() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dir_path = Utf8Path::from_path(dir.path()).expect("utf8");
        let pending = dir_path.join("missing.snap.new");

        let err = reject_pending(&pending).expect_err("missing pending snapshot should fail");

        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(
            err.to_string()
                .replace('\\', "/")
                .contains(&normalize_path(&pending)),
            "{err}"
        );
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

        let pending = find_pending_snapshots(dir_path).expect("find pending");
        assert_eq!(pending.len(), 2);
    }

    #[test]
    fn find_pending_reports_walk_errors() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dir_path = Utf8Path::from_path(dir.path()).expect("utf8");
        let root_file = dir_path.join("not-a-directory");
        std::fs::write(&root_file, "").expect("write");

        let err = find_pending_snapshots(&root_file).expect_err("walk should fail");
        assert_eq!(err.kind(), io::ErrorKind::NotADirectory);
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

        let snaps = find_snapshots(dir_path).expect("find snapshots");
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

        let unreferenced = find_unreferenced_snapshots(dir_path).expect("find unreferenced");
        assert_eq!(unreferenced.len(), 1);
        insta::assert_snapshot!(unreferenced[0].reason, @"test file not found: test.py");
    }

    #[test]
    fn unreferenced_invalid_source_metadata() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dir_path = Utf8Path::from_path(dir.path()).expect("utf8");
        let snap_dir = dir_path.join("snapshots");
        std::fs::create_dir_all(&snap_dir).expect("mkdir");

        let snapshot = SnapshotFile {
            metadata: crate::format::SnapshotMetadata {
                source: Some("not-a-source-reference".to_string()),
                ..Default::default()
            },
            content: "hello\n".to_string(),
        };
        write_snapshot(&snap_dir.join("test__test_foo.snap"), &snapshot).expect("write");

        let unreferenced = find_unreferenced_snapshots(dir_path).expect("find unreferenced");
        assert_eq!(unreferenced.len(), 1);
        insta::assert_snapshot!(
            unreferenced[0].reason,
            @"invalid source metadata: not-a-source-reference"
        );
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

        let unreferenced = find_unreferenced_snapshots(dir_path).expect("find unreferenced");
        assert_eq!(unreferenced.len(), 1);
        insta::assert_snapshot!(unreferenced[0].reason, @"function `test_foo` not found in test.py");
    }

    #[test]
    fn unreferenced_source_read_errors_are_reported() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dir_path = Utf8Path::from_path(dir.path()).expect("utf8");
        let snap_dir = dir_path.join("snapshots");
        std::fs::create_dir_all(&snap_dir).expect("mkdir");
        std::fs::create_dir(dir_path.join("test.py")).expect("create source directory");

        let snapshot = SnapshotFile {
            metadata: crate::format::SnapshotMetadata {
                source: Some("test.py:5::test_foo".to_string()),
                ..Default::default()
            },
            content: "hello\n".to_string(),
        };
        write_snapshot(&snap_dir.join("test__test_foo.snap"), &snapshot).expect("write");

        let err = find_unreferenced_snapshots(dir_path).expect_err("source read should fail");

        assert!(
            err.to_string().contains("failed to read source file"),
            "{err}"
        );
        assert!(err.to_string().contains("test.py"), "{err}");
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

        let unreferenced = find_unreferenced_snapshots(dir_path).expect("find unreferenced");
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

    #[test]
    fn remove_snapshot_leaves_non_empty_dir() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dir_path = Utf8Path::from_path(dir.path()).expect("utf8");
        let snap_dir = dir_path.join("snapshots");
        std::fs::create_dir_all(&snap_dir).expect("mkdir");

        let snap_path = snap_dir.join("test__test_foo.snap");
        let other_path = snap_dir.join("test__test_bar.snap");
        std::fs::write(&snap_path, "content").expect("write");
        std::fs::write(&other_path, "content").expect("write");

        remove_snapshot(&snap_path).expect("remove");
        assert!(!snap_path.exists());
        assert!(other_path.exists());
        assert!(snap_dir.exists());
    }
}
