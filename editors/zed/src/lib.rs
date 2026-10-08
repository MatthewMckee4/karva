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

    if let Some(path) = which("karva") {
        return Some(command_for_binary(path, &["server"], settings));
    }
    // Preserve project-local versions for existing UV workspaces.
    which("uv").map(|path| command_for_binary(path, &["run", "karva", "server"], settings))
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
        local_command(&settings, |name| worktree.which(name)).ok_or_else(|| {
            "Karva requires a local installation; install Karva with uv, put karva on the worktree PATH, or set lsp.karva.binary.path".to_owned()
        })
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
        command_for_binary, initialization_options, local_command, workspace_configuration,
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
    fn path_resolution_precedes_uv() {
        let settings = Default::default();
        let command =
            local_command(&settings, |name| Some(format!("/bin/{name}"))).expect("Karva command");
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
