//! Native coverage artifact combination transaction.

use std::collections::BTreeSet;
use std::io::ErrorKind;

use anyhow::{Context, Result, bail};
use camino::{Utf8Path, Utf8PathBuf};
use karva_cli::CoverageCombineCommand;
use karva_coverage::CoverageFilters;
use karva_coverage::native::NativeCoverage;
use karva_project::path::absolute;

pub(super) fn combine(
    args: &CoverageCombineCommand,
    project_root: &Utf8Path,
    data_file: &Utf8Path,
    filters: &CoverageFilters,
) -> Result<()> {
    let inputs = combine_inputs(args, project_root, data_file)?;
    let mut errors = Vec::new();
    let mut artifacts = Vec::new();
    for path in &inputs {
        match NativeCoverage::read(path).and_then(|artifact| filters.map_native_paths(artifact)) {
            Ok(artifact) => artifacts.push((path, artifact)),
            Err(error) => errors.push(format!("`{path}`: {error:#}")),
        }
    }

    let mut combined = if args.append && data_file.exists() {
        match NativeCoverage::read(data_file)
            .and_then(|artifact| filters.map_native_paths(artifact))
        {
            Ok(artifact) => Some(artifact),
            Err(error) => {
                errors.push(format!("`{data_file}`: {error:#}"));
                None
            }
        }
    } else {
        None
    };
    for (path, artifact) in artifacts {
        if let Some(current) = &combined {
            let mut candidate = current.clone();
            match candidate.merge(artifact) {
                Ok(()) => combined = Some(candidate),
                Err(error) => errors.push(format!("`{path}`: {error:#}")),
            }
        } else {
            combined = Some(artifact);
        }
    }
    if !errors.is_empty() {
        bail!(
            "failed to combine native coverage artifacts:\n{}",
            errors.join("\n")
        );
    }
    let Some(combined) = combined else {
        bail!("no native coverage artifacts found to combine");
    };
    combined.write(data_file)?;
    if !args.keep {
        for input in inputs {
            std::fs::remove_file(&input)
                .with_context(|| format!("combined coverage but failed to remove `{input}`"))?;
        }
    }
    Ok(())
}

fn combine_inputs(
    args: &CoverageCombineCommand,
    project_root: &Utf8Path,
    data_file: &Utf8Path,
) -> Result<Vec<Utf8PathBuf>> {
    let requested = if args.inputs.is_empty() {
        vec![
            data_file
                .parent()
                .unwrap_or_else(|| Utf8Path::new("."))
                .join("pending"),
        ]
    } else {
        args.inputs
            .iter()
            .map(|path| absolute(path, project_root))
            .collect()
    };
    let mut files = BTreeSet::new();
    for path in requested {
        collect_inputs(&path, &mut files)?;
    }
    files.remove(data_file);
    if files.is_empty() {
        bail!("no native coverage artifacts found to combine");
    }
    Ok(files.into_iter().collect())
}

fn collect_inputs(path: &Utf8Path, files: &mut BTreeSet<Utf8PathBuf>) -> Result<()> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => {
            files.insert(path.to_path_buf());
        }
        Ok(metadata) if metadata.is_dir() => {
            for entry in std::fs::read_dir(path)
                .with_context(|| format!("failed to read coverage artifact directory `{path}`"))?
            {
                let entry = entry.with_context(|| {
                    format!("failed to read coverage artifact directory `{path}`")
                })?;
                let child = Utf8PathBuf::from_path_buf(entry.path()).map_err(|path| {
                    anyhow::anyhow!(
                        "coverage artifact path contains non-Unicode characters: `{}`",
                        path.display()
                    )
                })?;
                if entry
                    .file_type()
                    .with_context(|| format!("failed to inspect coverage artifact `{child}`"))?
                    .is_dir()
                    || child.extension() == Some("json")
                {
                    collect_inputs(&child, files)?;
                }
            }
        }
        Ok(_) => bail!("coverage artifact input `{path}` is not a file or directory"),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            bail!("coverage artifact input `{path}` does not exist")
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect coverage artifact `{path}`"));
        }
    }
    Ok(())
}
