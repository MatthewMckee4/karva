use zed::settings::LspSettings;
use zed_extension_api as zed;

struct KarvaExtension;

fn command_for_binary(path: String, args: &[&str], settings: &LspSettings) -> zed::Command {
    let binary = settings.binary.as_ref();
    zed::Command {
        command: path,
        args: binary
            .and_then(|binary| binary.arguments.clone())
            .unwrap_or_else(|| args.iter().map(|arg| (*arg).to_owned()).collect()),
        env: binary
            .and_then(|binary| binary.env.as_ref())
            .map(|env| {
                env.iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn local_command(
    settings: &LspSettings,
    which: impl Fn(&str) -> Option<String>,
) -> Option<zed::Command> {
    if let Some(path) = settings
        .binary
        .as_ref()
        .and_then(|binary| binary.path.clone())
    {
        return Some(command_for_binary(path, &["server"], settings));
    }
    if let Some(path) = which("karva-language-server") {
        return Some(command_for_binary(path, &[], settings));
    }
    if let Some(path) = which("karva") {
        return Some(command_for_binary(path, &["server"], settings));
    }
    // Preserve project-local versions for existing UV workspaces.
    which("uv").map(|path| command_for_binary(path, &["run", "karva", "server"], settings))
}

fn asset_name(os: zed::Os, arch: zed::Architecture) -> zed::Result<String> {
    let target = match (os, arch) {
        (zed::Os::Mac, zed::Architecture::Aarch64) => "aarch64-apple-darwin",
        (zed::Os::Mac, zed::Architecture::X8664) => "x86_64-apple-darwin",
        (zed::Os::Linux, zed::Architecture::Aarch64) => "aarch64-unknown-linux-gnu",
        (zed::Os::Linux, zed::Architecture::X8664) => "x86_64-unknown-linux-gnu",
        (zed::Os::Windows, zed::Architecture::X8664) => "x86_64-pc-windows-msvc",
        _ => return Err("No managed Karva language server for this platform; set lsp.karva.binary.path or install Karva with uv".to_owned()),
    };
    let suffix = if os == zed::Os::Windows { ".exe" } else { "" };
    Ok(format!("karva-language-server-{target}{suffix}"))
}

fn cached_download(
    path: &std::path::Path,
    download: impl FnOnce(&str) -> zed::Result<()>,
    make_executable: impl FnOnce(&str) -> zed::Result<()>,
) -> zed::Result<String> {
    let binary = path
        .to_str()
        .ok_or("Karva cache path is not UTF-8")?
        .to_owned();
    if !path.is_file() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("Cannot create Karva cache: {error}"))?;
        }
        let partial = path.with_extension("partial");
        let partial_name = partial.to_str().ok_or("Karva download path is not UTF-8")?;
        download(partial_name).map_err(|error| format!("Cannot download Karva language server: {error}; set lsp.karva.binary.path to a local server"))?;
        make_executable(partial_name)?;
        std::fs::rename(&partial, path)
            .map_err(|error| format!("Cannot cache Karva language server: {error}"))?;
    } else {
        make_executable(&binary)?;
    }
    Ok(binary)
}

fn initialization_options(settings: &LspSettings) -> Option<zed::serde_json::Value> {
    settings.initialization_options.clone()
}

fn workspace_configuration(settings: &LspSettings) -> Option<zed::serde_json::Value> {
    settings.settings.clone()
}

impl zed::Extension for KarvaExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        let settings = LspSettings::for_worktree(language_server_id.as_ref(), worktree)
            .map_err(|error| format!("failed to read Karva language-server settings: {error}"))?;
        if let Some(command) = local_command(&settings, |name| worktree.which(name)) {
            return Ok(command);
        }
        let (os, arch) = zed::current_platform();
        let name = asset_name(os, arch)?;
        let release = zed::latest_github_release(
            "MatthewMckee4/karva",
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: true,
            },
        )
        .map_err(|error| {
            format!("Cannot find a Karva release: {error}; install uv or set lsp.karva.binary.path")
        })?;
        let asset = release.assets.iter().find(|asset| asset.name == name)
            .ok_or_else(|| format!("Karva release {} has no {name}; install uv and add Karva to the project, or set lsp.karva.binary.path", release.version))?;
        if release.version.contains(['/', '\\']) {
            return Err("Invalid Karva release version in download path".to_owned());
        }
        let path = std::path::Path::new("servers")
            .join(format!("karva-{}", release.version))
            .join(&name);
        let binary = cached_download(
            &path,
            |path| {
                zed::download_file(
                    &asset.download_url,
                    path,
                    zed::DownloadedFileType::Uncompressed,
                )
            },
            zed::make_file_executable,
        )?;
        Ok(command_for_binary(binary, &[], &settings))
    }

    fn language_server_initialization_options(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<Option<zed::serde_json::Value>> {
        let settings = LspSettings::for_worktree(language_server_id.as_ref(), worktree)
            .map_err(|error| format!("failed to read Karva language-server settings: {error}"))?;
        Ok(initialization_options(&settings))
    }

    fn language_server_workspace_configuration(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<Option<zed::serde_json::Value>> {
        let settings = LspSettings::for_worktree(language_server_id.as_ref(), worktree)
            .map_err(|error| format!("failed to read Karva language-server settings: {error}"))?;
        Ok(workspace_configuration(&settings))
    }
}

zed::register_extension!(KarvaExtension);

#[cfg(test)]
mod tests {
    use super::{
        asset_name, cached_download, command_for_binary, initialization_options, local_command,
        workspace_configuration,
    };
    use zed_extension_api as zed;

    #[test]
    fn runs_project_karva_server_through_uv() {
        let command = command_for_binary(
            "/resolved/uv".to_owned(),
            &["run", "karva", "server"],
            &Default::default(),
        );

        assert_eq!(command.command, "/resolved/uv");
        assert_eq!(command.args, ["run", "karva", "server"]);
        assert!(command.env.is_empty());
    }

    #[test]
    fn explicit_binary_wins_and_overrides_arguments_and_environment() {
        let settings: zed::settings::LspSettings = zed::serde_json::from_value(zed::serde_json::json!({
            "binary": {"path": "/custom/karva", "arguments": ["server"], "env": {"RUST_LOG": "debug"}}
        })).expect("settings");
        let command = local_command(&settings, |_| Some("/path/binary".to_owned()))
            .expect("explicit command");
        assert_eq!(command.command, "/custom/karva");
        assert_eq!(command.args, ["server"]);
        assert_eq!(command.env, [("RUST_LOG".to_owned(), "debug".to_owned())]);
    }

    #[test]
    fn path_resolution_precedes_uv_and_managed_downloads() {
        let settings = Default::default();
        let command = local_command(&settings, |name| Some(format!("/bin/{name}")))
            .expect("standalone command");
        assert_eq!(command.command, "/bin/karva-language-server");
        assert!(command.args.is_empty());
        let command = local_command(&settings, |name| {
            (name == "karva").then(|| "/bin/karva".to_owned())
        })
        .expect("Karva command");
        assert_eq!(command.args, ["server"]);
        let command = local_command(&settings, |name| {
            (name == "uv").then(|| "/bin/uv".to_owned())
        })
        .expect("UV command");
        assert_eq!(command.args, ["run", "karva", "server"]);
        assert!(local_command(&settings, |_| None).is_none());
    }

    #[rstest::rstest]
    #[case(zed::Os::Mac, zed::Architecture::Aarch64)]
    #[case(zed::Os::Mac, zed::Architecture::X8664)]
    #[case(zed::Os::Linux, zed::Architecture::Aarch64)]
    #[case(zed::Os::Linux, zed::Architecture::X8664)]
    #[case(zed::Os::Windows, zed::Architecture::X8664)]
    fn release_targets_match_packaging_matrix(
        #[case] os: zed::Os,
        #[case] arch: zed::Architecture,
    ) {
        let workflow = include_str!("../../../.github/workflows/build-language-server.yml");
        let name = asset_name(os, arch).expect("supported target");
        assert!(
            workflow.contains(&name),
            "asset missing from workflow: {name}"
        );
    }

    #[rstest::rstest]
    fn unsupported_platforms_explain_local_configuration(
        #[values(zed::Architecture::Aarch64, zed::Architecture::X86)] arch: zed::Architecture,
    ) {
        assert!(
            asset_name(zed::Os::Windows, arch)
                .expect_err("unsupported target")
                .contains("lsp.karva.binary.path")
        );
    }

    #[test]
    fn cached_download_is_reused_and_failed_downloads_are_not_promoted()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("release/server");
        let error = cached_download(
            &path,
            |partial| {
                std::fs::write(partial, b"incomplete").map_err(|error| error.to_string())?;
                Err("network failure".to_owned())
            },
            |_| Ok(()),
        )
        .expect_err("failed download");
        assert!(error.contains("network failure"));
        assert!(error.contains("lsp.karva.binary.path"));
        assert!(!path.exists());
        cached_download(
            &path,
            |partial| std::fs::write(partial, b"complete").map_err(|error| error.to_string()),
            |_| Ok(()),
        )?;
        cached_download(
            &path,
            |_| Err("must not download cached binary".to_owned()),
            |_| Ok(()),
        )?;
        assert_eq!(std::fs::read(&path)?, b"complete");
        Ok(())
    }

    #[test]
    fn forwards_lsp_initialization_and_workspace_settings() {
        let settings = zed::settings::LspSettings {
            binary: None,
            initialization_options: Some(zed::serde_json::json!({"profile": "ci"})),
            settings: Some(zed::serde_json::json!({"workspace": true})),
        };

        assert_eq!(
            initialization_options(&settings),
            Some(zed::serde_json::json!({"profile": "ci"}))
        );
        assert_eq!(
            workspace_configuration(&settings),
            Some(zed::serde_json::json!({"workspace": true}))
        );
    }

    #[test]
    fn registers_python_snippets_with_unique_karva_triggers() {
        let manifest = include_str!("../extension.toml");
        assert!(manifest.contains("snippets = [\"snippets/python.json\"]"));
        let snippets: zed::serde_json::Value =
            zed::serde_json::from_str(include_str!("../snippets/python.json"))
                .expect("valid snippet JSON");
        let mut prefixes = std::collections::HashSet::new();
        for snippet in snippets.as_object().expect("snippet map").values() {
            let prefix = snippet["prefix"].as_str().expect("snippet prefix");
            assert!(prefix.starts_with("karva_"));
            assert!(prefixes.insert(prefix), "duplicate trigger {prefix}");
            assert!(
                snippet["description"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty())
            );
            assert!(
                snippet["body"]
                    .as_array()
                    .is_some_and(|lines| !lines.is_empty()
                        && lines.iter().all(zed::serde_json::Value::is_string))
            );
        }
    }

    #[test]
    fn manifest_registers_karva_for_python() {
        let manifest = include_str!("../extension.toml");

        assert!(manifest.contains("id = \"karva\""));
        assert!(manifest.contains("[language_servers.karva]"));
        assert!(manifest.contains("languages = [\"Python\"]"));
        assert!(!manifest.contains("download_file"));
    }
}
