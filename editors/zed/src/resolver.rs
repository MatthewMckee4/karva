//! Karva language-server binary resolution.

use std::path::Path;

use zed_extension_api::{
    self as zed, Architecture, DownloadedFileType, LanguageServerId, Os, Worktree,
    set_language_server_installation_status,
};

const SERVER_NAME: &str = "karva-language-server";
const RELEASE_REPOSITORY: &str = "MatthewMckee4/karva";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlatformAsset {
    pub(crate) target: &'static str,
    pub(crate) archive_suffix: &'static str,
    pub(crate) binary_name: &'static str,
}

pub(crate) fn platform_asset(os: Os, architecture: Architecture) -> Result<PlatformAsset, String> {
    let target = match (os, architecture) {
        (Os::Mac, Architecture::Aarch64) => "aarch64-apple-darwin",
        (Os::Mac, Architecture::X8664) => "x86_64-apple-darwin",
        (Os::Linux, Architecture::Aarch64) => "aarch64-unknown-linux-gnu",
        (Os::Linux, Architecture::X8664) => "x86_64-unknown-linux-gnu",
        (Os::Windows, Architecture::Aarch64) => "aarch64-pc-windows-msvc",
        (Os::Windows, Architecture::X8664) => "x86_64-pc-windows-msvc",
        (Os::Mac, Architecture::X86) => return unsupported(os, architecture),
        (Os::Linux, Architecture::X86) => return unsupported(os, architecture),
        (Os::Windows, Architecture::X86) => return unsupported(os, architecture),
    };

    Ok(PlatformAsset {
        target,
        archive_suffix: if os == Os::Windows { "zip" } else { "tar.gz" },
        binary_name: if os == Os::Windows {
            "karva-language-server.exe"
        } else {
            SERVER_NAME
        },
    })
}

fn unsupported(os: Os, architecture: Architecture) -> Result<PlatformAsset, String> {
    Err(format!(
        "Karva language server has no release asset for {os:?}/{architecture:?}"
    ))
}

pub(crate) fn asset_name(asset: PlatformAsset) -> String {
    format!("{SERVER_NAME}-{}.{}", asset.target, asset.archive_suffix)
}

pub(crate) fn managed_binary_path(version: &str, asset: PlatformAsset) -> String {
    format!("karva-{version}/{}", asset.binary_name)
}

pub(crate) fn release_repository() -> &'static str {
    RELEASE_REPOSITORY
}

#[derive(Debug, Default)]
pub(crate) struct BinaryResolver {
    cached_binary_path: Option<String>,
}

impl BinaryResolver {
    pub(crate) fn resolve(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &Worktree,
        explicit_path: Option<String>,
    ) -> zed::Result<String> {
        if let Some(path) = select_configured_path(explicit_path, worktree.which(SERVER_NAME)) {
            return Ok(path);
        }

        if let Some(path) = &self.cached_binary_path {
            if is_file(path) {
                return Ok(path.clone());
            }
        }

        self.download(language_server_id)
    }

    fn download(&mut self, language_server_id: &LanguageServerId) -> zed::Result<String> {
        let (os, architecture) = zed::current_platform();
        let asset = platform_asset(os, architecture)?;
        set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );
        let release = zed::latest_github_release(
            release_repository(),
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )?;
        let archive_name = asset_name(asset);
        let archive = release
            .assets
            .iter()
            .find(|candidate| candidate.name == archive_name)
            .ok_or_else(|| {
                format!(
                    "Karva release {} has no asset {archive_name}; install {SERVER_NAME} manually or configure lsp.karva.binary.path",
                    release.version
                )
            })?;
        let version_dir = format!("karva-{}", release.version);
        let binary_path = managed_binary_path(&release.version, asset);
        if !is_file(&binary_path) {
            set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );
            let file_type = if asset.archive_suffix == "zip" {
                DownloadedFileType::Zip
            } else {
                DownloadedFileType::GzipTar
            };
            zed::download_file(&archive.download_url, &version_dir, file_type)
                .map_err(|error| format!("failed to download {archive_name}: {error}"))?;
        }

        if !is_file(&binary_path) {
            return Err(format!(
                "downloaded {archive_name}, but expected executable at {binary_path}; check release packaging or configure lsp.karva.binary.path"
            ));
        }
        if os != Os::Windows {
            zed::make_file_executable(&binary_path)
                .map_err(|error| format!("failed to make {binary_path} executable: {error}"))?;
        }
        self.cached_binary_path = Some(binary_path.clone());
        Ok(binary_path)
    }
}

fn select_configured_path(
    explicit_path: Option<String>,
    path_path: Option<String>,
) -> Option<String> {
    explicit_path.or(path_path)
}

pub(crate) fn is_file(path: &str) -> bool {
    Path::new(path).is_file()
}

#[cfg(test)]
mod tests {
    use super::{asset_name, managed_binary_path, platform_asset, select_configured_path};
    use zed_extension_api::{Architecture, Os};

    #[test]
    fn maps_supported_release_targets() {
        assert_eq!(
            platform_asset(Os::Mac, Architecture::Aarch64)
                .expect("macOS arm64 supported")
                .target,
            "aarch64-apple-darwin"
        );
        assert_eq!(
            platform_asset(Os::Linux, Architecture::X8664)
                .expect("Linux x64 supported")
                .target,
            "x86_64-unknown-linux-gnu"
        );
        assert_eq!(
            platform_asset(Os::Windows, Architecture::X8664)
                .expect("Windows x64 supported")
                .target,
            "x86_64-pc-windows-msvc"
        );
    }

    #[test]
    fn rejects_unsupported_release_targets() {
        let error = platform_asset(Os::Linux, Architecture::X86).expect_err("x86 unsupported");
        assert!(error.contains("no release asset"));
    }

    #[test]
    fn formats_archive_and_binary_paths() {
        let asset = platform_asset(Os::Mac, Architecture::Aarch64).expect("macOS arm64 supported");
        assert_eq!(
            asset_name(asset),
            "karva-language-server-aarch64-apple-darwin.tar.gz"
        );
        assert_eq!(
            managed_binary_path("0.2.0", asset),
            "karva-0.2.0/karva-language-server"
        );

        let asset =
            platform_asset(Os::Windows, Architecture::X8664).expect("Windows x64 supported");
        assert_eq!(
            asset_name(asset),
            "karva-language-server-x86_64-pc-windows-msvc.zip"
        );
        assert_eq!(
            managed_binary_path("0.2.0", asset),
            "karva-0.2.0/karva-language-server.exe"
        );
    }

    #[test]
    fn prefers_explicit_path_over_path_binary() {
        assert_eq!(
            select_configured_path(
                Some("/custom/karva".to_owned()),
                Some("/path/karva".to_owned())
            ),
            Some("/custom/karva".to_owned())
        );
        assert_eq!(
            select_configured_path(None, Some("/path/karva".to_owned())),
            Some("/path/karva".to_owned())
        );
        assert_eq!(select_configured_path(None, None), None);
    }
}
