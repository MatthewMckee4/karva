mod resolver;

use zed::settings::LspSettings;
use zed_extension_api as zed;

use crate::resolver::BinaryResolver;

struct KarvaExtension {
    resolver: BinaryResolver,
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
        let args = binary
            .and_then(|binary| binary.arguments.clone())
            .unwrap_or_default();
        let env = binary
            .and_then(|binary| binary.env.clone())
            .unwrap_or_default()
            .into_iter()
            .collect();
        Ok(zed::Command {
            command: path,
            args,
            env,
        })
    }
}

zed::register_extension!(KarvaExtension);
