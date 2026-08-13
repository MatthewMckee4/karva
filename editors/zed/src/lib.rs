use zed::settings::LspSettings;
use zed_extension_api as zed;

struct KarvaExtension;

fn command_for_uv(path: String) -> zed::Command {
    zed::Command {
        command: path,
        args: vec!["run".to_owned(), "karva".to_owned(), "server".to_owned()],
        env: Default::default(),
    }
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
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        let uv = worktree
            .which("uv")
            .ok_or_else(|| "Karva requires `uv` on the Zed worktree PATH".to_owned())?;
        Ok(command_for_uv(uv))
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
    use super::{command_for_uv, initialization_options, workspace_configuration};
    use zed_extension_api as zed;

    #[test]
    fn runs_project_karva_server_through_uv() {
        let command = command_for_uv("/resolved/uv".to_owned());

        assert_eq!(command.command, "/resolved/uv");
        assert_eq!(command.args, ["run", "karva", "server"]);
        assert!(command.env.is_empty());
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
    fn manifest_registers_karva_for_python() {
        let manifest = include_str!("../extension.toml");

        assert!(manifest.contains("id = \"karva\""));
        assert!(manifest.contains("[language_servers.karva]"));
        assert!(manifest.contains("languages = [\"Python\"]"));
        assert!(!manifest.contains("download_file"));
    }
}
