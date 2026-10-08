//! Project-local source candidates for explicit Python imports.

use camino::{Utf8Path, Utf8PathBuf};
use ruff_python_ast::StmtImportFrom;

/// Returns package and module source candidates without reading or importing Python.
///
/// Absolute imports search the project root, which Karva adds to Python's
/// import path. Relative imports
/// ascend from the importing directory. Candidates never escape `project_root`.
/// Callers select the first source available in their immutable workspace snapshot.
pub fn project_import_paths(
    project_root: &Utf8Path,
    importing_path: &Utf8Path,
    import: &StmtImportFrom,
) -> Vec<Utf8PathBuf> {
    let Some(directory) = importing_path.parent() else {
        return Vec::new();
    };
    let mut roots = Vec::new();
    if import.level == 0 {
        roots.push(project_root);
    } else {
        let mut root = directory;
        for _ in 1..import.level {
            let Some(parent) = root.parent() else {
                return Vec::new();
            };
            root = parent;
        }
        roots.push(root);
    }
    roots
        .into_iter()
        .filter(|root| root.starts_with(project_root))
        .flat_map(|root| {
            let mut path = root.to_path_buf();
            if let Some(module) = &import.module {
                for component in module.as_str().split('.') {
                    path.push(component);
                }
            }
            let package = path.join("__init__.py");
            let module = path.with_extension("py");
            if import.module.is_none() {
                vec![package]
            } else {
                vec![package, module]
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use camino::{Utf8Path, Utf8PathBuf};
    use ruff_python_ast::Stmt;
    use ruff_python_parser::{Mode, parse};

    use super::project_import_paths;

    fn paths(path: &str, source: &str) -> Vec<Utf8PathBuf> {
        let parsed = parse(source, Mode::Module.into()).expect("valid import");
        let module = parsed.try_into_module().expect("module");
        let import = module
            .syntax()
            .body
            .iter()
            .find_map(|statement| {
                if let Stmt::ImportFrom(import) = statement {
                    Some(import)
                } else {
                    None
                }
            })
            .expect("expected import");
        project_import_paths(Utf8Path::new("/project"), Utf8Path::new(path), import)
    }

    #[test]
    fn relative_import_candidates_stay_inside_project() {
        assert_eq!(
            paths("/project/pkg/conftest.py", "from .support import resource"),
            [
                Utf8PathBuf::from("/project/pkg/support/__init__.py"),
                Utf8PathBuf::from("/project/pkg/support.py")
            ]
        );
        assert!(paths("/project/conftest.py", "from ..support import resource").is_empty());
    }
}
