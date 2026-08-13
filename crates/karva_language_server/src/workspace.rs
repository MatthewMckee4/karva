//! Karva project discovery isolated by LSP workspace folder.

use std::collections::HashMap;
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use karva_metadata::{
    Options, ProjectMetadata, ProjectMetadataError, ProjectOptionsOverrides, UnknownProfile,
};
use karva_project::Project;
use lsp_types::{Uri, WorkspaceFolder};
use ruff_python_ast::PythonVersion;

/// Failure to map an editor document to resolved Karva configuration.
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    /// The client sent a file URI whose path is not UTF-8.
    #[error("URI does not contain a UTF-8 file path: {0}")]
    NonUtf8FileUri(Uri),

    /// The client sent a URI that does not identify a local file.
    #[error("URI does not identify a local file: {0}")]
    NotAFileUri(Uri),

    /// The file URI has no containing directory.
    #[error("document has no containing directory: {0}")]
    MissingParent(Utf8PathBuf),

    /// Karva configuration discovery failed.
    #[error(transparent)]
    Metadata(#[from] ProjectMetadataError),

    /// The configured profile does not exist.
    #[error(transparent)]
    UnknownProfile(#[from] UnknownProfile),
}

#[derive(Debug)]
struct Workspace {
    root: Utf8PathBuf,
    projects: HashMap<Utf8PathBuf, Arc<Project>>,
}

impl Workspace {
    fn new(root: Utf8PathBuf) -> Self {
        Self {
            root,
            projects: HashMap::new(),
        }
    }

    fn project_for_path(
        &mut self,
        path: &Utf8Path,
        python_version: PythonVersion,
        profile: Option<&str>,
    ) -> Result<Arc<Project>, WorkspaceError> {
        let project = discover_project(path, &self.root, python_version, profile)?;
        let root = project.cwd().clone();
        if let Some(project) = self.projects.get(&root) {
            return Ok(Arc::clone(project));
        }
        let project = Arc::new(project);
        self.projects.insert(root, Arc::clone(&project));
        Ok(project)
    }
}

/// Owned inputs for resolving one document's Karva project off the event loop.
#[derive(Clone, Debug)]
pub struct PreparedProjectDiscovery {
    path: Utf8PathBuf,
    workspace_root: Utf8PathBuf,
    python_version: PythonVersion,
    profile: Option<String>,
}

impl PreparedProjectDiscovery {
    /// Reads project configuration and applies initialization overrides.
    pub fn discover(self) -> Result<Project, WorkspaceError> {
        discover_project(
            &self.path,
            &self.workspace_root,
            self.python_version,
            self.profile.as_deref(),
        )
    }
}

fn discover_project(
    path: &Utf8Path,
    workspace_root: &Utf8Path,
    python_version: PythonVersion,
    profile: Option<&str>,
) -> Result<Project, WorkspaceError> {
    let directory = path
        .parent()
        .ok_or_else(|| WorkspaceError::MissingParent(path.to_path_buf()))?;
    let mut metadata = ProjectMetadata::discover(directory, python_version)?;
    if metadata.root() == directory
        && !directory.join("karva.toml").exists()
        && !directory.join("pyproject.toml").exists()
    {
        let fallback_root = directory
            .ancestors()
            .take_while(|ancestor| ancestor.starts_with(workspace_root))
            .find(|ancestor| ancestor.join(".git").exists())
            .unwrap_or(workspace_root);
        metadata = ProjectMetadata::discover(fallback_root, python_version)?;
    }
    metadata.apply_overrides(
        &ProjectOptionsOverrides::new(None, Options::default())
            .with_profile(profile.map(str::to_owned)),
    )?;
    Ok(Project::from_metadata(metadata))
}

/// Independent project caches for every editor workspace folder.
#[derive(Debug)]
pub struct Workspaces {
    folders: Vec<WorkspaceFolder>,
    profile: Option<String>,
    python_version: PythonVersion,
    roots: Vec<Workspace>,
}

impl Workspaces {
    pub fn new(
        folders: Vec<WorkspaceFolder>,
        python_version: PythonVersion,
        profile: Option<String>,
    ) -> Result<Self, WorkspaceError> {
        let roots = folders
            .iter()
            .map(|folder| uri_to_path(&folder.uri).map(Workspace::new))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            folders,
            profile,
            python_version,
            roots,
        })
    }

    pub fn folders(&self) -> impl Iterator<Item = &WorkspaceFolder> {
        self.folders.iter()
    }

    pub fn project_for_uri(&mut self, uri: &Uri) -> Result<Arc<Project>, WorkspaceError> {
        let path = uri_to_path(uri)?;
        let workspace = self
            .roots
            .iter_mut()
            .filter(|workspace| path.starts_with(&workspace.root))
            .max_by_key(|workspace| workspace.root.components().count());

        if let Some(workspace) = workspace {
            return workspace.project_for_path(&path, self.python_version, self.profile.as_deref());
        }

        let root = path
            .parent()
            .ok_or_else(|| WorkspaceError::MissingParent(path.clone()))?
            .to_path_buf();
        let mut workspace = Workspace::new(root);
        let project =
            workspace.project_for_path(&path, self.python_version, self.profile.as_deref())?;
        self.roots.push(workspace);
        Ok(project)
    }

    /// Captures project-discovery inputs without reading the filesystem.
    pub fn prepare_project_discovery(
        &self,
        uri: &Uri,
    ) -> Result<PreparedProjectDiscovery, WorkspaceError> {
        let path = uri_to_path(uri)?;
        let workspace_root = self
            .roots
            .iter()
            .filter(|workspace| path.starts_with(&workspace.root))
            .max_by_key(|workspace| workspace.root.components().count())
            .map(|workspace| workspace.root.clone())
            .or_else(|| path.parent().map(Utf8Path::to_path_buf))
            .ok_or_else(|| WorkspaceError::MissingParent(path.clone()))?;
        Ok(PreparedProjectDiscovery {
            path,
            workspace_root,
            python_version: self.python_version,
            profile: self.profile.clone(),
        })
    }

    pub fn open_folder(&mut self, folder: WorkspaceFolder) -> Result<(), WorkspaceError> {
        if self.folders.iter().any(|open| open.uri == folder.uri) {
            return Ok(());
        }
        let root = uri_to_path(&folder.uri)?;
        self.folders.push(folder);
        self.roots.push(Workspace::new(root));
        Ok(())
    }

    pub fn close_folder(&mut self, uri: &Uri) -> Result<(), WorkspaceError> {
        let root = uri_to_path(uri)?;
        self.folders.retain(|folder| &folder.uri != uri);
        self.roots.retain(|workspace| workspace.root != root);
        Ok(())
    }

    pub fn configuration_changed(&mut self, uri: &Uri) -> Result<(), WorkspaceError> {
        let path = uri_to_path(uri)?;
        if !matches!(path.file_name(), Some("karva.toml" | "pyproject.toml")) {
            return Ok(());
        }

        for workspace in &mut self.roots {
            if path.starts_with(&workspace.root) {
                workspace.projects.clear();
            }
        }
        Ok(())
    }
}

pub fn uri_to_path(uri: &Uri) -> Result<Utf8PathBuf, WorkspaceError> {
    let path = uri
        .to_file_path()
        .map_err(|()| WorkspaceError::NotAFileUri(uri.clone()))?;
    Utf8PathBuf::from_path_buf(path).map_err(|_| WorkspaceError::NonUtf8FileUri(uri.clone()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use ruff_python_ast::PythonVersion;

    use super::*;

    fn root(temp_dir: &tempfile::TempDir) -> Utf8PathBuf {
        let root = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf())
            .expect("temp directory should be UTF-8");
        fs::create_dir(root.join(".git")).expect("create project boundary");
        root
    }

    fn file_uri(path: &Utf8Path) -> Uri {
        Uri::from_file_path(path).expect("test path should produce a file URI")
    }

    fn folder(root: &Utf8Path, name: &str) -> WorkspaceFolder {
        WorkspaceFolder {
            uri: file_uri(root),
            name: name.to_owned(),
        }
    }

    fn prefix(project: &Project) -> &str {
        &project.settings().test().test_function_prefix
    }

    #[test]
    fn discovers_karva_toml_for_nested_document() {
        let temp_dir = tempfile::tempdir().expect("create temp directory");
        let root = root(&temp_dir);
        let nested = root.join("src/package");
        fs::create_dir_all(&nested).expect("create nested source directory");
        fs::write(
            root.join("karva.toml"),
            "[profile.default.test]\ntest-function-prefix = \"spec_\"\n",
        )
        .expect("write Karva configuration");
        let mut workspaces =
            Workspaces::new(vec![folder(&root, "root")], PythonVersion::PY311, None)
                .expect("create workspaces");

        let project = workspaces
            .project_for_uri(&file_uri(&nested.join("test_example.py")))
            .expect("discover project");

        assert_eq!(project.cwd(), &root);
        assert_eq!(prefix(&project), "spec_");
    }

    #[test]
    fn discovers_pyproject_configuration() {
        let temp_dir = tempfile::tempdir().expect("create temp directory");
        let root = root(&temp_dir);
        fs::write(
            root.join("pyproject.toml"),
            "[tool.karva.profile.default.test]\ntest-function-prefix = \"check_\"\n",
        )
        .expect("write Python project configuration");
        let mut workspaces =
            Workspaces::new(vec![folder(&root, "root")], PythonVersion::PY311, None)
                .expect("create workspaces");

        let project = workspaces
            .project_for_uri(&file_uri(&root.join("test_example.py")))
            .expect("discover project");

        assert_eq!(prefix(&project), "check_");
    }

    #[test]
    fn selects_initialization_profile() {
        let temp_dir = tempfile::tempdir().expect("create temp directory");
        let root = root(&temp_dir);
        fs::write(
            root.join("karva.toml"),
            "[profile.default.test]\ntest-function-prefix = \"test_\"\n\n[profile.ci.test]\ntest-function-prefix = \"ci_\"\n",
        )
        .expect("write profiled configuration");
        let mut workspaces = Workspaces::new(
            vec![folder(&root, "root")],
            PythonVersion::PY311,
            Some("ci".to_owned()),
        )
        .expect("create workspaces");

        let project = workspaces
            .project_for_uri(&file_uri(&root.join("test_example.py")))
            .expect("discover project");

        assert_eq!(prefix(&project), "ci_");
    }

    #[test]
    fn closest_nested_configuration_wins() {
        let temp_dir = tempfile::tempdir().expect("create temp directory");
        let root = root(&temp_dir);
        let nested = root.join("packages/child");
        fs::create_dir_all(&nested).expect("create nested project");
        fs::write(
            root.join("karva.toml"),
            "[profile.default.test]\ntest-function-prefix = \"root_\"\n",
        )
        .expect("write root configuration");
        fs::write(
            nested.join("karva.toml"),
            "[profile.default.test]\ntest-function-prefix = \"child_\"\n",
        )
        .expect("write nested configuration");
        let mut workspaces =
            Workspaces::new(vec![folder(&root, "root")], PythonVersion::PY311, None)
                .expect("create workspaces");

        let project = workspaces
            .project_for_uri(&file_uri(&nested.join("test_example.py")))
            .expect("discover nested project");

        assert_eq!(project.cwd(), &nested);
        assert_eq!(prefix(&project), "child_");
    }

    #[test]
    fn missing_configuration_uses_defaults() {
        let temp_dir = tempfile::tempdir().expect("create temp directory");
        let root = root(&temp_dir);
        let mut workspaces =
            Workspaces::new(vec![folder(&root, "root")], PythonVersion::PY311, None)
                .expect("create workspaces");

        let nested = root.join("src/package");
        fs::create_dir_all(&nested).expect("create nested source directory");
        let project = workspaces
            .project_for_uri(&file_uri(&nested.join("test_example.py")))
            .expect("create default project");

        assert_eq!(project.cwd(), &root);
        assert_eq!(prefix(&project), "test");
    }

    #[test]
    fn resolves_configured_test_overrides() {
        let temp_dir = tempfile::tempdir().expect("create temp directory");
        let root = root(&temp_dir);
        fs::write(
            root.join("karva.toml"),
            "[[profile.default.overrides]]\nfilter = \"tag(network)\"\nretries = 1\n",
        )
        .expect("write override configuration");
        let mut workspaces =
            Workspaces::new(vec![folder(&root, "root")], PythonVersion::PY311, None)
                .expect("create workspaces");

        let project = workspaces
            .project_for_uri(&file_uri(&root.join("test_example.py")))
            .expect("discover project");

        assert_eq!(project.settings().overrides().len(), 1);
        assert_eq!(project.settings().overrides()[0].retries, Some(1));
    }

    #[test]
    fn workspace_folders_keep_projects_independent() {
        let first_dir = tempfile::tempdir().expect("create first temp directory");
        let second_dir = tempfile::tempdir().expect("create second temp directory");
        let first = root(&first_dir);
        let second = root(&second_dir);
        fs::write(
            first.join("karva.toml"),
            "[profile.default.test]\ntest-function-prefix = \"first_\"\n",
        )
        .expect("write first configuration");
        fs::write(
            second.join("karva.toml"),
            "[profile.default.test]\ntest-function-prefix = \"second_\"\n",
        )
        .expect("write second configuration");
        let mut workspaces = Workspaces::new(
            vec![folder(&first, "first"), folder(&second, "second")],
            PythonVersion::PY311,
            None,
        )
        .expect("create workspaces");

        let first_project = workspaces
            .project_for_uri(&file_uri(&first.join("test_example.py")))
            .expect("discover first project");
        let second_project = workspaces
            .project_for_uri(&file_uri(&second.join("test_example.py")))
            .expect("discover second project");

        assert_eq!(prefix(&first_project), "first_");
        assert_eq!(prefix(&second_project), "second_");
        assert!(!Arc::ptr_eq(&first_project, &second_project));
    }

    #[test]
    fn configuration_change_invalidates_cached_project() {
        let temp_dir = tempfile::tempdir().expect("create temp directory");
        let root = root(&temp_dir);
        let config = root.join("karva.toml");
        fs::write(
            &config,
            "[profile.default.test]\ntest-function-prefix = \"before_\"\n",
        )
        .expect("write initial configuration");
        let mut workspaces =
            Workspaces::new(vec![folder(&root, "root")], PythonVersion::PY311, None)
                .expect("create workspaces");
        let document = file_uri(&root.join("test_example.py"));
        let before = workspaces
            .project_for_uri(&document)
            .expect("discover initial project");
        fs::write(
            &config,
            "[profile.default.test]\ntest-function-prefix = \"after_\"\n",
        )
        .expect("write updated configuration");

        workspaces
            .configuration_changed(&file_uri(&config))
            .expect("invalidate project");
        let after = workspaces
            .project_for_uri(&document)
            .expect("rediscover project");

        assert_eq!(prefix(&before), "before_");
        assert_eq!(prefix(&after), "after_");
        assert!(!Arc::ptr_eq(&before, &after));
    }
}
