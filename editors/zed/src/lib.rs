use std::path::Path;

use semver::Version;
use zed::settings::LspSettings;
use zed_extension_api as zed;

struct KarvaExtension {
    cached_binary: Option<String>,
}

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

fn release_asset<'a>(
    release: &'a zed::GithubRelease,
    name: &str,
) -> zed::Result<&'a zed::GithubReleaseAsset> {
    release.assets.iter().find(|asset| asset.name == name)
        .ok_or_else(|| format!("Karva release {} has no {name}; install uv and add Karva to the project, or set lsp.karva.binary.path", release.version))
}

fn release_version(value: &str) -> zed::Result<Version> {
    Version::parse(value.strip_prefix('v').unwrap_or(value))
        .map_err(|error| format!("Invalid Karva release version {value}: {error}"))
}

fn latest_release(
    mut lookup: impl FnMut(bool) -> zed::Result<zed::GithubRelease>,
) -> zed::Result<zed::GithubRelease> {
    let versioned = |release: zed::GithubRelease| {
        release_version(&release.version).map(|version| (version, release))
    };
    // Zed filters by an exact prerelease flag rather than including both kinds.
    let stable = lookup(false).and_then(versioned);
    let prerelease = lookup(true).and_then(versioned);
    match (stable, prerelease) {
        (Ok((stable_version, stable)), Ok((prerelease_version, prerelease))) => {
            Ok(if stable_version >= prerelease_version {
                stable
            } else {
                prerelease
            })
        }
        (Ok((_, release)), Err(_)) | (Err(_), Ok((_, release))) => Ok(release),
        (Err(stable), Err(prerelease)) => Err(format!(
            "Cannot find a Karva release: {stable}; {prerelease}; install uv or set lsp.karva.binary.path"
        )),
    }
}

fn find_cached_binary(root: &Path, asset: &str) -> Option<(Version, String)> {
    std::fs::read_dir(root)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let version = release_version(name.to_str()?.strip_prefix("karva-")?).ok()?;
            let binary = entry.path().join(asset);
            binary
                .is_file()
                .then(|| binary.to_str().map(|path| (version, path.to_owned())))
                .flatten()
        })
        .max_by(|left, right| left.0.cmp(&right.0))
}

fn managed_binary(
    root: &Path,
    asset_name: &str,
    lookup: impl FnMut(bool) -> zed::Result<zed::GithubRelease>,
    download: impl FnOnce(&str, &str) -> zed::Result<()>,
    make_executable: impl Fn(&str) -> zed::Result<()>,
) -> zed::Result<String> {
    let cached = find_cached_binary(root, asset_name);
    let resolved = (|| {
        let release = latest_release(lookup)?;
        let version = release_version(&release.version)?;
        if let Some((cached_version, path)) = &cached {
            if *cached_version >= version {
                make_executable(path)?;
                return Ok(path.clone());
            }
        }
        let asset = release_asset(&release, asset_name)?;
        let path = root
            .join(format!("karva-{}", release.version))
            .join(asset_name);
        cached_download(
            &path,
            |path| download(&asset.download_url, path),
            &make_executable,
        )
    })();
    match resolved {
        Ok(binary) => Ok(binary),
        Err(error) => {
            let Some((_, binary)) = cached else {
                return Err(error);
            };
            make_executable(&binary)
                .map_err(|error| format!("Cannot run cached Karva server at {binary}: {error}"))?;
            Ok(binary)
        }
    }
}

fn cached_download(
    path: &Path,
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
        make_executable(partial_name).map_err(|error| {
            format!("Cannot make Karva server executable at {partial_name}: {error}")
        })?;
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
        Self {
            cached_binary: None,
        }
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
        if let Some(binary) = &self.cached_binary {
            if Path::new(binary).is_file() {
                return Ok(command_for_binary(binary.clone(), &[], &settings));
            }
        }
        let (os, arch) = zed::current_platform();
        let name = asset_name(os, arch)?;
        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );
        let binary = managed_binary(
            Path::new("servers"),
            &name,
            |pre_release| {
                zed::latest_github_release(
                    "MatthewMckee4/karva",
                    zed::GithubReleaseOptions {
                        require_assets: true,
                        pre_release,
                    },
                )
            },
            |url, path| {
                zed::set_language_server_installation_status(
                    language_server_id,
                    &zed::LanguageServerInstallationStatus::Downloading,
                );
                zed::download_file(url, path, zed::DownloadedFileType::Uncompressed)
            },
            zed::make_file_executable,
        )?;
        self.cached_binary = Some(binary.clone());
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
        asset_name, cached_download, command_for_binary, initialization_options, latest_release,
        local_command, managed_binary, release_asset, workspace_configuration,
    };
    use std::path::Path;
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
    fn path_resolution_precedes_uv() {
        let settings = Default::default();
        let command =
            local_command(&settings, |name| Some(format!("/bin/{name}"))).expect("PATH command");
        assert_eq!(command.command, "/bin/karva");
        assert_eq!(command.args, ["server"]);
        let command = local_command(&settings, |name| {
            (name == "uv").then(|| "/bin/uv".to_owned())
        })
        .expect("UV command");
        assert_eq!(command.args, ["run", "karva", "server"]);
        assert!(local_command(&settings, |_| None).is_none());
    }

    #[test]
    fn explicit_binary_defaults_to_server_without_path_lookup() {
        let settings = zed::serde_json::from_value(zed::serde_json::json!({
            "binary": {"path": "/custom/karva"}
        }))
        .expect("settings");
        let command = local_command(&settings, |_| None).expect("explicit command");
        assert_eq!(command.command, "/custom/karva");
        assert_eq!(command.args, ["server"]);
    }

    #[test]
    fn uv_resolution_honors_arguments_and_environment() {
        let settings = zed::serde_json::from_value(zed::serde_json::json!({
            "binary": {"arguments": ["run", "karva", "server"], "env": {"RUST_LOG": "debug"}}
        }))
        .expect("settings");
        let command = local_command(&settings, |name| {
            (name == "uv").then(|| "/bin/uv".to_owned())
        })
        .expect("UV command");
        assert_eq!(command.command, "/bin/uv");
        assert_eq!(command.args, ["run", "karva", "server"]);
        assert_eq!(command.env, [("RUST_LOG".to_owned(), "debug".to_owned())]);
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
    fn missing_release_assets_explain_the_release_target_and_local_override() {
        let release = zed::GithubRelease {
            version: "0.0.1-alpha.13".to_owned(),
            assets: vec![],
        };
        let error = release_asset(&release, "karva-language-server-aarch64-apple-darwin")
            .expect_err("missing native asset");
        assert!(error.contains("0.0.1-alpha.13"));
        assert!(error.contains("aarch64-apple-darwin"));
        assert!(error.contains("lsp.karva.binary.path"));
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
    }

    #[rstest::rstest]
    #[case(Some("0.1.0"), Some("0.0.2-alpha.1"), "0.1.0")]
    #[case(Some("0.1.0"), Some("0.2.0-alpha.1"), "0.2.0-alpha.1")]
    #[case(None, Some("0.0.1-alpha.13"), "0.0.1-alpha.13")]
    #[case(Some("v1.0.0"), None, "v1.0.0")]
    fn chooses_the_newest_available_release_kind(
        #[case] stable: Option<&str>,
        #[case] prerelease: Option<&str>,
        #[case] expected: &str,
    ) {
        let mut queried = Vec::new();
        let release = latest_release(|pre| {
            queried.push(pre);
            if pre { prerelease } else { stable }
                .map(|version| zed::GithubRelease {
                    version: version.to_owned(),
                    assets: vec![],
                })
                .ok_or_else(|| "no release of this kind".to_owned())
        })
        .expect("one release kind available");
        assert_eq!(queried, [false, true]);
        assert_eq!(release.version, expected);
    }

    #[test]
    fn offline_lookup_reuses_the_newest_matching_complete_cache()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let asset = "karva-language-server-aarch64-apple-darwin";
        for (version, filename) in [
            ("0.2.0", asset),
            ("0.10.0", asset),
            ("0.99.0", "other-platform"),
            ("0.98.0", "server.partial"),
            ("not-a-version", asset),
        ] {
            let root = directory.path().join(format!("karva-{version}"));
            std::fs::create_dir_all(&root)?;
            std::fs::write(root.join(filename), b"complete")?;
        }
        let binary = managed_binary(
            directory.path(),
            asset,
            |_| Err("offline".to_owned()),
            |_, _| Err("must not download".to_owned()),
            |_| Ok(()),
        )
        .expect("existing cache works offline");
        let expected = directory.path().join("karva-0.10.0").join(asset);
        assert_eq!(Path::new(&binary), expected);
        Ok(())
    }

    #[test]
    fn offline_without_a_cached_binary_explains_local_configuration() {
        let directory = tempfile::tempdir().expect("cache root");
        let error = managed_binary(
            directory.path(),
            "server",
            |_| Err("offline".to_owned()),
            |_, _| Err("must not download".to_owned()),
            |_| Ok(()),
        )
        .expect_err("no available binary");
        assert!(error.contains("offline"));
        assert!(error.contains("lsp.karva.binary.path"));
    }

    #[rstest::rstest]
    fn cached_binary_survives_missing_assets_and_failed_downloads(
        #[values(false, true)] fail_download: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let asset = "server";
        let cached = directory.path().join("karva-1.0.0").join(asset);
        std::fs::create_dir_all(cached.parent().expect("cache parent"))?;
        std::fs::write(&cached, b"complete")?;
        let attempted = std::cell::Cell::new(false);
        let binary = managed_binary(
            directory.path(),
            asset,
            |_| {
                Ok(zed::GithubRelease {
                    version: "2.0.0".to_owned(),
                    assets: if fail_download {
                        vec![zed::GithubReleaseAsset {
                            name: asset.to_owned(),
                            download_url: "https://example.invalid/server".to_owned(),
                        }]
                    } else {
                        vec![]
                    },
                })
            },
            |_, partial| {
                attempted.set(true);
                std::fs::write(partial, b"incomplete").map_err(|error| error.to_string())?;
                Err("offline".to_owned())
            },
            |_| Ok(()),
        )
        .expect("cached version survives update failure");
        assert_eq!(Path::new(&binary), cached);
        assert_eq!(attempted.get(), fail_download);
        assert!(!directory.path().join("karva-2.0.0").join(asset).is_file());
        Ok(())
    }

    #[test]
    fn a_partial_release_lookup_never_downgrades_a_newer_cache()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let cached = directory.path().join("karva-1.0.0/server");
        std::fs::create_dir_all(cached.parent().expect("cache parent"))?;
        std::fs::write(&cached, b"complete")?;
        let binary = managed_binary(
            directory.path(),
            "server",
            |pre| {
                if pre {
                    Ok(zed::GithubRelease {
                        version: "0.9.0-alpha.1".to_owned(),
                        assets: vec![],
                    })
                } else {
                    Err("offline".to_owned())
                }
            },
            |_, _| Err("must not download".to_owned()),
            |_| Ok(()),
        )
        .expect("newer cached binary");
        assert_eq!(Path::new(&binary), cached);
        Ok(())
    }
}
