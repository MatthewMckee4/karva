mod resolver;

use zed::settings::LspSettings;
use zed_extension_api as zed;

use crate::resolver::BinaryResolver;

struct KarvaExtension {
    resolver: BinaryResolver,
}

fn command_from_settings(
    path: String,
    binary: Option<&zed::settings::CommandSettings>,
) -> zed::Command {
    let args = binary
        .and_then(|binary| binary.arguments.clone())
        .unwrap_or_default();
    let env = binary
        .and_then(|binary| binary.env.clone())
        .unwrap_or_default()
        .into_iter()
        .collect();
    zed::Command {
        command: path,
        args,
        env,
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
        Self {
            resolver: BinaryResolver::default(),
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        let settings = LspSettings::for_worktree(language_server_id.as_ref(), worktree)
            .map_err(|error| format!("failed to read Karva language-server settings: {error}"))?;
        let binary = settings.binary.as_ref();
        let path = self.resolver.resolve(
            language_server_id,
            worktree,
            binary.and_then(|binary| binary.path.clone()),
        )?;
        Ok(command_from_settings(path, binary))
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
    use std::collections::HashMap;

    use super::{command_from_settings, initialization_options, workspace_configuration};
    use zed_extension_api as zed;

    #[test]
    fn forwards_binary_arguments_and_environment() {
        let settings = zed::settings::CommandSettings {
            path: Some("/configured/karva-language-server".to_owned()),
            arguments: Some(vec!["--stdio".to_owned()]),
            env: Some(HashMap::from([(
                "KARVA_LOG".to_owned(),
                "debug".to_owned(),
            )])),
        };
        let command = command_from_settings(
            "/resolved/karva-language-server".to_owned(),
            Some(&settings),
        );

        assert_eq!(command.command, "/resolved/karva-language-server");
        assert_eq!(command.args, ["--stdio"]);
        assert_eq!(command.env, [("KARVA_LOG".to_owned(), "debug".to_owned())]);
    }

    #[test]
    fn defaults_command_arguments_and_environment() {
        let command = command_from_settings("karva-language-server".to_owned(), None);

        assert_eq!(command.command, "karva-language-server");
        assert!(command.args.is_empty());
        assert!(command.env.is_empty());
    }

    #[test]
    fn forwards_lsp_initialization_and_workspace_settings() {
        let settings = zed::settings::LspSettings {
            binary: None,
            initialization_options: Some(zed::serde_json::json!({"logLevel": "debug"})),
            settings: Some(zed::serde_json::json!({"workspace": true})),
        };

        assert_eq!(
            initialization_options(&settings),
            Some(zed::serde_json::json!({"logLevel": "debug"}))
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
        assert!(manifest.contains("kind = \"download_file\""));
        assert!(manifest.contains("host = \"github.com\""));
    }
}
