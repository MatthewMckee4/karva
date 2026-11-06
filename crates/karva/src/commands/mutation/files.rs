//! Source discovery, complete-input cache identity, and atomic source application.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::Write;

use anyhow::{Context, Result, bail};
use camino::{Utf8Path, Utf8PathBuf};
use karva_mutation::{Mutant, MutationResults, fingerprint, fingerprint_file};
use karva_project::Project;
use pyo3::prelude::*;
use pyo3::types::PyModule;
use tempfile::NamedTempFile;

/// Serializes mutation commands within one project, without preventing result reads.
pub(super) fn lock(root: &Utf8Path) -> Result<File> {
    let directory = root.join(".karva/mutation");
    std::fs::create_dir_all(&directory)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join("lock"))?;
    file.try_lock().map_err(|error| {
        anyhow::anyhow!("another mutation command holds `{directory}/lock`: {error}")
    })?;
    Ok(file)
}

fn generated(name: &str) -> bool {
    name == karva_cache::CACHE_DIR
        || matches!(
            name,
            ".git"
                | ".karva"
                | ".venv"
                | "venv"
                | ".tox"
                | ".nox"
                | "__pycache__"
                | ".pytest_cache"
                | ".ruff_cache"
                | ".mypy_cache"
                | ".worktrees"
                | "target"
        )
}

/// Collects deterministic file paths without following directory symlinks.
///
/// Ignore rules are used for mutable source discovery, but deliberately bypassed
/// when fingerprinting test inputs so ignored configuration/data cannot go stale.
pub(super) fn files(
    root: &Utf8Path,
    sources: &[String],
    respect_ignore: bool,
) -> Result<Vec<Utf8PathBuf>> {
    let mut paths = BTreeSet::new();
    for source in sources {
        let source = root.join(source);
        if !source.exists() {
            bail!(
                "mutation source `{source}` does not exist; supply an existing file or directory"
            );
        }
        let mut walk = ignore::WalkBuilder::new(&source);
        walk.hidden(false)
            .git_ignore(respect_ignore)
            .git_global(respect_ignore)
            .git_exclude(respect_ignore)
            .ignore(respect_ignore)
            .parents(respect_ignore)
            .filter_entry(|entry| !entry.file_name().to_str().is_some_and(generated));
        for entry in walk.build() {
            let entry = entry.with_context(|| format!("cannot walk mutation source `{source}`"))?;
            if entry.file_type().is_some_and(|kind| kind.is_file()) {
                let path =
                    Utf8PathBuf::from_path_buf(entry.path().to_path_buf()).map_err(|path| {
                        anyhow::anyhow!("mutation source path is not Unicode: {}", path.display())
                    })?;
                let canonical = canonical_source(root, &path)?;
                paths.insert(canonical);
            }
        }
    }
    Ok(paths.into_iter().collect())
}

/// Rejects paths escaping the project, including source symlinks targeting outside it.
pub(super) fn canonical_source(root: &Utf8Path, path: &Utf8Path) -> Result<Utf8PathBuf> {
    let canonical = Utf8PathBuf::from_path_buf(std::fs::canonicalize(path)?)
        .map_err(|path| anyhow::anyhow!("non-Unicode mutation source: {}", path.display()))?;
    let canonical_root = std::fs::canonicalize(root)?;
    if !canonical.as_std_path().starts_with(canonical_root) {
        bail!("mutation source `{path}` is outside project root `{root}`");
    }
    Ok(canonical)
}

/// Portable source identity, independent of the platform's path separator.
pub(super) fn relative(root: &Utf8Path, path: &Utf8Path) -> Result<Utf8PathBuf> {
    let canonical_root = Utf8PathBuf::from_path_buf(std::fs::canonicalize(root)?)
        .map_err(|path| anyhow::anyhow!("non-Unicode root: {}", path.display()))?;
    Ok(path
        .strip_prefix(canonical_root)?
        .as_str()
        .replace('\\', "/")
        .into())
}

/// Includes all non-generated project bytes, installed dependency bytes, runtime,
/// environment, effective settings, and both independently linked executables.
pub(super) fn context(project: &Project, configuration: &serde_json::Value) -> Result<String> {
    let mut identities = Vec::new();
    for path in files(project.cwd(), &[".".into()], false)? {
        let relative = relative(project.cwd(), &path)?;
        let content = fingerprint_file(&path)
            .with_context(|| format!("cannot fingerprint mutation input `{path}`"))?;
        identities.push(fingerprint(&[
            relative.as_str().as_bytes(),
            content.as_bytes(),
        ]));
    }
    let executable = Utf8PathBuf::from_path_buf(std::env::current_exe()?)
        .map_err(|path| anyhow::anyhow!("non-Unicode executable: {}", path.display()))?;
    let worker = karva_runner::find_karva_worker_binary(project.cwd())?;
    identities.push(fingerprint(&[
        fingerprint_file(&executable)?.as_bytes(),
        fingerprint_file(&worker)?.as_bytes(),
    ]));
    identities.push(serde_json::to_string(&project.metadata().options)?);
    identities.push(serde_json::to_string(configuration)?);
    let source = std::ffi::CString::new(include_str!("environment.py"))?;
    let environment: String = Python::attach(|py| {
        let module = PyModule::from_code(
            py,
            &source,
            c"karva_mutation_environment.py",
            c"_karva_mutation_environment",
        )?;
        module.getattr("fingerprint")?.call0()?.extract()
    })?;
    identities.push(environment);
    Ok(fingerprint(
        &identities.iter().map(String::as_bytes).collect::<Vec<_>>(),
    ))
}

/// Checks the source again immediately before replacement and preserves permissions.
pub(super) fn apply(root: &Utf8Path, mutant: &Mutant) -> Result<()> {
    let path = canonical_source(root, &root.join(&mutant.path))?;
    let original = std::fs::read_to_string(&path)?;
    let replacement = mutant.apply(&original)?;
    let mut temporary = NamedTempFile::new_in(path.parent().context("source parent")?)?;
    temporary
        .as_file()
        .set_permissions(std::fs::metadata(&path)?.permissions())?;
    temporary.write_all(replacement.as_bytes())?;
    temporary.as_file().sync_all()?;
    if std::fs::read_to_string(&path)? != original {
        bail!(
            "mutant {}: `{path}` changed during application; source left unchanged",
            mutant.id
        );
    }
    temporary.persist(&path).map_err(|error| error.error)?;
    Ok(())
}

/// Removes records whose original source changed or disappeared before a bulk retest.
pub(super) fn prune_stale(root: &Utf8Path, results: &mut MutationResults) -> Result<()> {
    let mut sources = BTreeMap::new();
    for record in results.records.values() {
        if !sources.contains_key(&record.mutant.path) {
            let path = root.join(&record.mutant.path);
            let identity = match std::fs::read(&path) {
                Ok(bytes) => Some(fingerprint(&[&bytes])),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("cannot validate mutation source `{path}`"));
                }
            };
            sources.insert(record.mutant.path.clone(), identity);
        }
    }
    results.records.retain(|_, record| {
        sources.get(&record.mutant.path).and_then(Option::as_deref)
            == Some(record.mutant.source_fingerprint.as_str())
    });
    Ok(())
}
