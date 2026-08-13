//! Resolve, verify, and install the Karva language-server binary.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use tar::Archive;
use zed_extension_api::{
    self as zed, Architecture, DownloadedFileType, LanguageServerId, Os, Worktree,
    set_language_server_installation_status,
};
use zip::ZipArchive;

const SERVER_NAME: &str = "karva-language-server";
const RELEASE_REPOSITORY: &str = "MatthewMckee4/karva";
const RELEASE_MARKER: &str = ".karva-release";
const RELEASE_MARKER_HEADER: &str = "karva-zed-release-v1";
static NEXT_DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlatformAsset {
    pub(crate) target: &'static str,
    pub(crate) archive_suffix: &'static str,
    pub(crate) binary_name: &'static str,
}

/// Select published target; Zed reports no libc, so Linux always uses GNU.
pub(crate) fn platform_asset(os: Os, architecture: Architecture) -> Result<PlatformAsset, String> {
    let target = match (os, architecture) {
        (Os::Mac, Architecture::Aarch64) => "aarch64-apple-darwin",
        (Os::Mac, Architecture::X8664) => "x86_64-apple-darwin",
        (Os::Linux, Architecture::Aarch64) => "aarch64-unknown-linux-gnu",
        (Os::Linux, Architecture::X8664) => "x86_64-unknown-linux-gnu",
        (Os::Linux, Architecture::X86) => "i686-unknown-linux-gnu",
        (Os::Windows, Architecture::Aarch64) => "aarch64-pc-windows-msvc",
        (Os::Windows, Architecture::X8664) => "x86_64-pc-windows-msvc",
        (Os::Windows, Architecture::X86) => "i686-pc-windows-msvc",
        (Os::Mac, Architecture::X86) => {
            return Err(format!(
                "Karva language server has no release asset for {os:?}/{architecture:?}"
            ));
        }
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

pub(crate) fn asset_name(asset: PlatformAsset) -> String {
    format!("{SERVER_NAME}-{}.{}", asset.target, asset.archive_suffix)
}

fn install_directory_name(version: &str, asset: PlatformAsset) -> Result<String, String> {
    validate_release_version(version)?;
    let serial = NEXT_DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    Ok(format!(
        ".karva-{version}-{}.install-{timestamp}-{serial}",
        asset.target
    ))
}

fn create_install_directory_in(
    root: &Path,
    version: &str,
    asset: PlatformAsset,
) -> Result<String, String> {
    loop {
        let path = root.join(install_directory_name(version, asset)?);
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path.to_string_lossy().into_owned()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(format!(
                    "failed to create Karva install directory {}: {error}",
                    path.display()
                ));
            }
        }
    }
}

fn installed_binary_path(directory: &Path, asset: PlatformAsset) -> Result<String, String> {
    directory
        .join(asset.binary_name)
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| {
            format!(
                "Karva install path {} is not valid UTF-8",
                directory.display()
            )
        })
}

fn install_directory_matches(name: &str, version: &str, asset: PlatformAsset) -> bool {
    let prefix = format!(".karva-{version}-{}.install-", asset.target);
    let Some(sequence) = name.strip_prefix(&prefix) else {
        return false;
    };
    let mut fields = sequence.split('-');
    fields
        .next()
        .is_some_and(|field| !field.is_empty() && field.bytes().all(|byte| byte.is_ascii_digit()))
        && fields.next().is_some_and(|field| {
            !field.is_empty() && field.bytes().all(|byte| byte.is_ascii_digit())
        })
        && fields.next().is_none()
}

fn find_cached_install(
    root: &Path,
    version: &str,
    asset: PlatformAsset,
) -> Result<Option<String>, String> {
    let mut candidates = Vec::new();
    for entry in fs::read_dir(root)
        .map_err(|error| format!("failed to list Karva install directory: {error}"))?
    {
        let entry =
            entry.map_err(|error| format!("failed to read Karva install entry: {error}"))?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if install_directory_matches(name, version, asset) && is_directory(&path) {
            candidates.push(path);
        }
    }
    candidates.sort_unstable();
    for path in candidates {
        if is_valid_release_directory(&path, version, asset) {
            return installed_binary_path(&path, asset).map(Some);
        }
    }
    Ok(None)
}

fn validate_release_version(version: &str) -> Result<(), String> {
    let invalid = version.is_empty()
        || matches!(version, "." | "..")
        || version.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
        });
    if invalid {
        return Err(format!(
            "Karva release version/tag {version:?} is not a safe path component; configure lsp.karva.binary.path or use a release with a simple tag"
        ));
    }
    Ok(())
}

fn digest_hex(digest: [u8; 32]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn write_release_marker(
    path: &Path,
    version: &str,
    asset: PlatformAsset,
    digest: [u8; 32],
) -> Result<(), String> {
    validate_release_version(version)?;
    let mut marker = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| {
            format!(
                "failed to create Karva release marker {}: {error}",
                path.display()
            )
        })?;
    marker
        .write_all(
            format!(
                "{RELEASE_MARKER_HEADER}\nversion={version}\nasset={}\nbinary={}\nsha256={}\n",
                asset_name(asset),
                asset.binary_name,
                digest_hex(digest)
            )
            .as_bytes(),
        )
        .map_err(|error| {
            format!(
                "failed to write Karva release marker {}: {error}",
                path.display()
            )
        })?;
    marker.flush().map_err(|error| {
        format!(
            "failed to flush Karva release marker {}: {error}",
            path.display()
        )
    })?;
    marker.sync_all().map_err(|error| {
        format!(
            "failed to sync Karva release marker {}: {error}",
            path.display()
        )
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReleaseMarker {
    version: String,
    archive: String,
    binary: String,
    digest: [u8; 32],
}

fn parse_release_marker(path: &Path) -> Option<ReleaseMarker> {
    if !is_directory(path)
        || !path
            .join(RELEASE_MARKER)
            .to_str()
            .is_some_and(is_regular_file)
    {
        return None;
    }
    let contents = fs::read_to_string(path.join(RELEASE_MARKER)).ok()?;
    let mut lines = contents.strip_suffix('\n')?.lines();
    if lines.next()? != RELEASE_MARKER_HEADER {
        return None;
    }
    let version = lines.next()?.strip_prefix("version=")?;
    let archive = lines.next()?.strip_prefix("asset=")?;
    let binary = lines.next()?.strip_prefix("binary=")?;
    let digest = parse_digest(lines.next()?.strip_prefix("sha256=")?)?;
    if lines.next().is_some() || validate_release_version(version).is_err() {
        return None;
    }
    Some(ReleaseMarker {
        version: version.to_owned(),
        archive: archive.to_owned(),
        binary: binary.to_owned(),
        digest,
    })
}

fn is_valid_release_directory(path: &Path, version: &str, asset: PlatformAsset) -> bool {
    let Some(marker) = parse_release_marker(path) else {
        return false;
    };
    let binary = path.join(asset.binary_name);
    let Some(binary) = binary.to_str() else {
        return false;
    };
    marker.version == version
        && marker.archive == asset_name(asset)
        && marker.binary == asset.binary_name
        && is_regular_file(binary)
        && sha256_digest(binary, asset.binary_name).is_ok_and(|digest| digest == marker.digest)
}

#[derive(Debug, Default)]
pub(crate) struct BinaryResolver {
    cached_binary_path: Option<String>,
    cached_binary_digest: Option<[u8; 32]>,
}

impl BinaryResolver {
    pub(crate) fn resolve(
        &mut self,
        id: &LanguageServerId,
        worktree: &Worktree,
        explicit_path: Option<String>,
    ) -> zed::Result<String> {
        match configured_binary_path(explicit_path, worktree.which(SERVER_NAME)) {
            Ok(Some(path)) => {
                set_status(id, &zed::LanguageServerInstallationStatus::None);
                return Ok(path);
            }
            Ok(None) => {}
            Err(error) => return fail(id, error),
        }
        if let Some(path) = self.cached_binary() {
            set_status(id, &zed::LanguageServerInstallationStatus::None);
            return Ok(path);
        }
        self.download(id)
    }

    fn download(&mut self, id: &LanguageServerId) -> zed::Result<String> {
        set_status(
            id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );
        match self.download_inner(id) {
            Ok(path) => {
                set_status(id, &zed::LanguageServerInstallationStatus::None);
                Ok(path)
            }
            Err(error) => fail(id, error),
        }
    }

    fn download_inner(&mut self, id: &LanguageServerId) -> zed::Result<String> {
        let (os, architecture) = zed::current_platform();
        let asset = platform_asset(os, architecture)?;
        let release = zed::latest_github_release(
            RELEASE_REPOSITORY,
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: true,
            },
        )?;
        validate_release_version(&release.version)?;
        let archive_name = asset_name(asset);
        let archive = release.assets.iter().find(|candidate| candidate.name == archive_name).ok_or_else(|| format!("Karva release {} has no asset {archive_name}; install {SERVER_NAME} manually or configure lsp.karva.binary.path", release.version))?;
        if let Some(binary_path) = find_cached_install(Path::new("."), &release.version, asset)? {
            let digest = sha256_digest(&binary_path, asset.binary_name)?;
            self.cache_binary(binary_path.clone(), digest);
            return Ok(binary_path);
        }
        set_status(id, &zed::LanguageServerInstallationStatus::Downloading);
        let checksum = find_checksum_asset(&release.assets, asset)?;
        let binary_path = install_release(&release.version, asset, archive, checksum, os)?;
        let digest = sha256_digest(&binary_path, asset.binary_name)?;
        self.cache_binary(binary_path.clone(), digest);
        Ok(binary_path)
    }

    fn cached_binary(&self) -> Option<String> {
        let path = self.cached_binary_path.as_deref()?;
        let expected = self.cached_binary_digest?;
        if is_regular_file(path)
            && sha256_digest(path, SERVER_NAME).is_ok_and(|actual| actual == expected)
        {
            Some(path.to_owned())
        } else {
            None
        }
    }

    fn cache_binary(&mut self, path: String, digest: [u8; 32]) {
        self.cached_binary_path = Some(path);
        self.cached_binary_digest = Some(digest);
    }
}

fn install_release(
    version: &str,
    asset: PlatformAsset,
    archive: &zed::GithubReleaseAsset,
    checksum: &zed::GithubReleaseAsset,
    os: Os,
) -> Result<String, String> {
    let install_directory = create_install_directory_in(Path::new("."), version, asset)?;
    install_release_inner(version, &install_directory, asset, archive, checksum, os)
        .and_then(|()| installed_binary_path(Path::new(&install_directory), asset))
        .map_err(|error| {
            format!(
                "{error}; incomplete Karva install remains at {install_directory}; remove it manually after verifying no Karva install is active"
            )
        })
}

fn install_release_inner(
    version: &str,
    install_directory: &str,
    asset: PlatformAsset,
    archive: &zed::GithubReleaseAsset,
    checksum: &zed::GithubReleaseAsset,
    os: Os,
) -> Result<(), String> {
    let archive_name = asset_name(asset);
    let checksum_path = Path::new(install_directory).join(format!("{}.sha256", archive_name));
    zed::download_file(
        &checksum.download_url,
        &checksum_path.to_string_lossy(),
        DownloadedFileType::Uncompressed,
    )
    .map_err(|error| format!("failed to download {}: {error}", checksum.name))?;
    let expected = read_expected_digest(&checksum_path.to_string_lossy(), &archive_name)?;
    let archive_path = Path::new(install_directory).join(&archive_name);
    zed::download_file(
        &archive.download_url,
        &archive_path.to_string_lossy(),
        DownloadedFileType::Uncompressed,
    )
    .map_err(|error| format!("failed to download {archive_name}: {error}"))?;
    verify_sha256(&archive_path.to_string_lossy(), expected, &archive_name)?;
    extract_archive(&archive_path.to_string_lossy(), install_directory, asset)?;
    let binary = Path::new(install_directory).join(asset.binary_name);
    if !binary.to_str().is_some_and(is_regular_file) {
        return Err(format!(
            "downloaded {archive_name}, but it has no regular {} entry",
            asset.binary_name
        ));
    }
    if os != Os::Windows {
        zed::make_file_executable(binary.to_string_lossy().as_ref())
            .map_err(|error| format!("failed to make {} executable: {error}", binary.display()))?;
    }
    let digest = sha256_digest(&binary.to_string_lossy(), asset.binary_name)?;
    write_release_marker(
        &Path::new(install_directory).join(RELEASE_MARKER),
        version,
        asset,
        digest,
    )
}

fn set_status(id: &LanguageServerId, status: &zed::LanguageServerInstallationStatus) {
    set_language_server_installation_status(id, status);
}

fn fail<T>(id: &LanguageServerId, error: String) -> zed::Result<T> {
    set_status(
        id,
        &zed::LanguageServerInstallationStatus::Failed(error.clone()),
    );
    Err(error)
}

fn validate_explicit_path(path: &str) -> Result<(), String> {
    if Path::new(path).is_file() {
        Ok(())
    } else {
        Err(format!(
            "lsp.karva.binary.path does not point to a file: {path}; set it to a Karva language-server executable, remove it to use PATH, or leave it unset to download a release"
        ))
    }
}

fn configured_binary_path(
    explicit: Option<String>,
    path_binary: Option<String>,
) -> Result<Option<String>, String> {
    if let Some(path) = explicit {
        validate_explicit_path(&path)?;
        return Ok(Some(path));
    }
    Ok(path_binary)
}

fn find_checksum_asset(
    assets: &[zed::GithubReleaseAsset],
    asset: PlatformAsset,
) -> Result<&zed::GithubReleaseAsset, String> {
    let name = format!("{}.sha256", asset_name(asset));
    assets.iter().find(|candidate| candidate.name == name).ok_or_else(|| format!("Karva release is missing checksum asset {name}; install {SERVER_NAME} manually or configure lsp.karva.binary.path"))
}

fn read_expected_digest(path: &str, archive_name: &str) -> Result<[u8; 32], String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("failed to read checksum {path}: {error}"))?;
    let line = contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .find(|line| {
            let mut fields = line.split_whitespace();
            fields.next();
            fields
                .next()
                .and_then(|name| Path::new(name.trim_start_matches('*')).file_name())
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == archive_name)
        })
        .ok_or_else(|| format!("checksum {path} has no entry for {archive_name}"))?;
    let digest = line
        .split_whitespace()
        .next()
        .ok_or_else(|| format!("checksum {path} has malformed entry for {archive_name}"))?;
    parse_digest(digest)
        .ok_or_else(|| format!("checksum {path} has invalid SHA-256 digest for {archive_name}"))
}

fn parse_digest(digest: &str) -> Option<[u8; 32]> {
    let bytes = digest.as_bytes();
    if bytes.len() != 64 || !bytes.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    let mut output = [0; 32];
    for (index, pair) in bytes.chunks_exact(2).enumerate() {
        output[index] = hex_pair(pair[0], pair[1]);
    }
    Some(output)
}

fn hex_pair(high: u8, low: u8) -> u8 {
    fn digit(value: u8) -> u8 {
        match value {
            b'0'..=b'9' => value - b'0',
            b'a'..=b'f' => value - b'a' + 10,
            b'A'..=b'F' => value - b'A' + 10,
            _ => 0,
        }
    }
    digit(high) * 16 + digit(low)
}

fn verify_sha256(path: &str, expected: [u8; 32], name: &str) -> Result<(), String> {
    if sha256_digest(path, name)? == expected {
        Ok(())
    } else {
        Err(format!(
            "checksum verification failed for {name}; remove the cached release and retry"
        ))
    }
}

fn sha256_digest(path: &str, name: &str) -> Result<[u8; 32], String> {
    let mut file = File::open(path).map_err(|error| format!("failed to open {path}: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 16 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("failed to hash {name}: {error}"))?;
        if count == 0 {
            return Ok(hasher.finalize().into());
        }
        hasher.update(&buffer[..count]);
    }
}

fn extract_archive(
    archive_path: &str,
    destination: &str,
    asset: PlatformAsset,
) -> Result<(), String> {
    let expected = Path::new(asset.binary_name);
    match asset.archive_suffix {
        "tar.gz" => {
            let file = File::open(archive_path)
                .map_err(|error| format!("failed to open {archive_path}: {error}"))?;
            let mut archive = Archive::new(GzDecoder::new(file));
            let mut found = false;
            for entry in archive
                .entries()
                .map_err(|error| format!("failed to read {archive_path}: {error}"))?
            {
                let mut entry = entry
                    .map_err(|error| format!("failed to read {archive_path} entry: {error}"))?;
                let path = entry
                    .path()
                    .map_err(|error| format!("failed to read {archive_path} entry path: {error}"))?
                    .into_owned();
                if path != expected {
                    continue;
                }
                if found || !entry.header().entry_type().is_file() {
                    return Err(format!(
                        "{archive_path} has duplicate or non-regular {} entry",
                        asset.binary_name
                    ));
                }
                extract_entry(&mut entry, &Path::new(destination).join(asset.binary_name))?;
                found = true;
            }
            if found {
                Ok(())
            } else {
                Err(format!(
                    "{archive_path} has no regular {} entry",
                    asset.binary_name
                ))
            }
        }
        "zip" => {
            let file = File::open(archive_path)
                .map_err(|error| format!("failed to open {archive_path}: {error}"))?;
            let mut archive = ZipArchive::new(file)
                .map_err(|error| format!("failed to read {archive_path}: {error}"))?;
            let mut entry = archive.by_name(asset.binary_name).map_err(|error| {
                format!(
                    "{archive_path} has no regular {} entry: {error}",
                    asset.binary_name
                )
            })?;
            if entry.is_dir() {
                return Err(format!(
                    "{archive_path} entry {} is a directory",
                    asset.binary_name
                ));
            }
            extract_entry(&mut entry, &Path::new(destination).join(asset.binary_name))
        }
        suffix => Err(format!("unsupported Karva archive format: {suffix}")),
    }
}

fn extract_entry(reader: &mut impl Read, output: &Path) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output)
        .map_err(|error| format!("failed to create {}: {error}", output.display()))?;
    io::copy(reader, &mut file)
        .map_err(|error| format!("failed to extract {}: {error}", output.display()))?;
    file.sync_all()
        .map_err(|error| format!("failed to sync {}: {error}", output.display()))
}

fn is_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.file_type().is_dir() && !metadata.file_type().is_symlink())
}

fn is_regular_file(path: &str) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.file_type().is_file() && !metadata.file_type().is_symlink())
}

#[cfg(test)]
mod tests {
    use std::{
        fs::File,
        io::Write,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use flate2::{Compression, write::GzEncoder};
    use sha2::{Digest, Sha256};
    use zed_extension_api::{Architecture, Os};

    use super::{
        NEXT_DIRECTORY_SEQUENCE, RELEASE_MARKER, asset_name, configured_binary_path,
        create_install_directory_in, digest_hex, extract_archive, find_cached_install,
        install_directory_name, is_valid_release_directory, platform_asset, read_expected_digest,
        validate_explicit_path, validate_release_version, verify_sha256, write_release_marker,
    };

    #[test]
    fn maps_published_targets() {
        let mac = platform_asset(Os::Mac, Architecture::Aarch64).expect("mac arm");
        let windows = platform_asset(Os::Windows, Architecture::Aarch64).expect("windows arm");
        assert_eq!(
            asset_name(mac),
            "karva-language-server-aarch64-apple-darwin.tar.gz"
        );
        assert_eq!(
            asset_name(windows),
            "karva-language-server-aarch64-pc-windows-msvc.zip"
        );
        assert!(platform_asset(Os::Mac, Architecture::X86).is_err());
    }

    #[test]
    fn precedence_and_explicit_path_errors_are_actionable() {
        let root = test_directory("precedence");
        let explicit = root.join("custom");
        std::fs::write(&explicit, b"binary").unwrap();
        let explicit = explicit.to_string_lossy().into_owned();
        assert_eq!(
            configured_binary_path(Some(explicit.clone()), Some("PATH".into())).unwrap(),
            Some(explicit)
        );
        assert_eq!(
            configured_binary_path(None, Some("PATH".into())).unwrap(),
            Some("PATH".into())
        );
        assert!(
            validate_explicit_path("missing")
                .unwrap_err()
                .contains("lsp.karva.binary.path")
        );
        remove_test_directory(&root);
    }

    #[test]
    fn validates_tags_and_generates_unique_wasi_safe_install_names() {
        for value in ["", ".", "..", "../escape", r"..\escape", "release\0tag"] {
            assert!(validate_release_version(value).is_err());
        }
        let asset = platform_asset(Os::Mac, Architecture::Aarch64).unwrap();
        let first = install_directory_name("v0.2.0-alpha.1", asset).unwrap();
        let second = install_directory_name("v0.2.0-alpha.1", asset).unwrap();
        assert_ne!(first, second);
        assert!(first.starts_with(".karva-v0.2.0-alpha.1-aarch64-apple-darwin.install-"));
        assert_eq!(
            first.rsplit_once(".install-").unwrap().1.split('-').count(),
            2
        );
    }

    #[test]
    fn parses_and_verifies_checksum_sidecar() {
        let root = test_directory("checksum");
        let archive = root.join("archive.tar.gz");
        std::fs::write(&archive, b"archive").unwrap();
        let digest = digest_hex(Sha256::digest(b"archive").into());
        let sidecar = root.join("checksums");
        std::fs::write(&sidecar, format!("{digest}  archive.tar.gz\n")).unwrap();
        let expected = read_expected_digest(sidecar.to_str().unwrap(), "archive.tar.gz").unwrap();
        verify_sha256(archive.to_str().unwrap(), expected, "archive.tar.gz").unwrap();
        std::fs::write(&archive, b"changed").unwrap();
        assert!(verify_sha256(archive.to_str().unwrap(), expected, "archive.tar.gz").is_err());
        remove_test_directory(&root);
    }

    #[test]
    fn extracts_only_exact_binary_path() {
        let root = test_directory("extract");
        let archive_path = root.join("archive.tar.gz");
        let output = File::create(&archive_path).unwrap();
        let mut encoder = GzEncoder::new(output, Compression::default());
        append_raw_tar_file(&mut encoder, b"../escape", b"bad");
        append_raw_tar_file(&mut encoder, b"karva-language-server", b"good");
        encoder.write_all(&[0; 1024]).unwrap();
        encoder.finish().unwrap();
        let destination = root.join("install");
        std::fs::create_dir(&destination).unwrap();
        let asset = platform_asset(Os::Mac, Architecture::Aarch64).unwrap();
        extract_archive(
            archive_path.to_str().unwrap(),
            destination.to_str().unwrap(),
            asset,
        )
        .unwrap();
        assert_eq!(
            std::fs::read(destination.join(asset.binary_name)).unwrap(),
            b"good"
        );
        assert!(!root.join("escape").exists());
        remove_test_directory(&root);
    }

    #[test]
    fn scanner_uses_valid_install_and_preserves_tampered_one() {
        let root = test_directory("cache");
        let asset = platform_asset(Os::Mac, Architecture::Aarch64).unwrap();
        let invalid = create_install_directory_in(&root, "0.2.0", asset).unwrap();
        write_install_binary(&invalid, b"tampered");
        let valid = create_install_directory_in(&root, "0.2.0", asset).unwrap();
        write_valid_install(&valid, "0.2.0", asset, b"valid");
        let found = find_cached_install(&root, "0.2.0", asset).unwrap().unwrap();
        assert_eq!(Path::new(&found).parent().unwrap(), Path::new(&valid));
        assert!(Path::new(&invalid).exists());
        assert!(!is_valid_release_directory(
            Path::new(&invalid),
            "0.2.0",
            asset
        ));
        remove_test_directory(&root);
    }

    #[test]
    fn independent_installs_never_overwrite_or_delete_winner() {
        let root = test_directory("concurrent");
        let asset = platform_asset(Os::Mac, Architecture::Aarch64).unwrap();
        let first = create_install_directory_in(&root, "0.2.0", asset).unwrap();
        let second = create_install_directory_in(&root, "0.2.0", asset).unwrap();
        write_valid_install(&first, "0.2.0", asset, b"first");
        write_valid_install(&second, "0.2.0", asset, b"second");
        let found = find_cached_install(&root, "0.2.0", asset).unwrap().unwrap();
        assert!(
            found == format!("{first}/{}", asset.binary_name)
                || found == format!("{second}/{}", asset.binary_name)
        );
        assert_eq!(
            std::fs::read(Path::new(&first).join(asset.binary_name)).unwrap(),
            b"first"
        );
        assert_eq!(
            std::fs::read(Path::new(&second).join(asset.binary_name)).unwrap(),
            b"second"
        );
        remove_test_directory(&root);
    }

    #[test]
    fn incomplete_install_remains_visible_for_manual_cleanup() {
        let root = test_directory("interrupted");
        let asset = platform_asset(Os::Mac, Architecture::Aarch64).unwrap();
        let install = create_install_directory_in(&root, "0.2.0", asset).unwrap();
        write_install_binary(&install, b"partial");
        assert!(
            find_cached_install(&root, "0.2.0", asset)
                .unwrap()
                .is_none()
        );
        assert!(Path::new(&install).exists());
        remove_test_directory(&root);
    }

    #[cfg(unix)]
    #[test]
    fn scanner_does_not_follow_path_swap_symlink() {
        use std::os::unix::fs::symlink;

        let root = test_directory("path-swap");
        let external = test_directory("external");
        let asset = platform_asset(Os::Mac, Architecture::Aarch64).unwrap();
        let external_binary = external.join(asset.binary_name);
        std::fs::write(&external_binary, b"external").unwrap();
        let name = install_directory_name("0.2.0", asset).unwrap();
        symlink(&external, root.join(name)).unwrap();
        assert!(
            find_cached_install(&root, "0.2.0", asset)
                .unwrap()
                .is_none()
        );
        assert_eq!(std::fs::read(external_binary).unwrap(), b"external");
        remove_test_directory(&root);
        remove_test_directory(&external);
    }

    #[test]
    fn release_marker_is_immutable_and_cache_requires_matching_digest() {
        let root = test_directory("marker");
        let asset = platform_asset(Os::Mac, Architecture::Aarch64).unwrap();
        let directory = create_install_directory_in(&root, "0.2.0", asset).unwrap();
        write_valid_install(&directory, "0.2.0", asset, b"binary");
        let marker = Path::new(&directory).join(RELEASE_MARKER);
        assert!(write_release_marker(&marker, "0.2.0", asset, [0; 32]).is_err());
        assert!(is_valid_release_directory(
            Path::new(&directory),
            "0.2.0",
            asset
        ));
        std::fs::write(Path::new(&directory).join(asset.binary_name), b"changed").unwrap();
        assert!(!is_valid_release_directory(
            Path::new(&directory),
            "0.2.0",
            asset
        ));
        remove_test_directory(&root);
    }

    fn append_raw_tar_file(writer: &mut GzEncoder<File>, path: &[u8], contents: &[u8]) {
        let mut header = [0; 512];
        header[..path.len()].copy_from_slice(path);
        write_octal(&mut header[100..108], 0o755);
        write_octal(&mut header[124..136], contents.len() as u64);
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        header[148..156].fill(b' ');
        let checksum = header.iter().map(|byte| u64::from(*byte)).sum();
        write_octal(&mut header[148..156], checksum);
        writer.write_all(&header).unwrap();
        writer.write_all(contents).unwrap();
        writer
            .write_all(&vec![0; (512 - contents.len() % 512) % 512])
            .unwrap();
    }

    fn write_octal(field: &mut [u8], value: u64) {
        let text = format!("{value:0width$o}\0", width = field.len() - 1);
        field.copy_from_slice(text.as_bytes());
    }

    fn write_install_binary(path: &str, contents: &[u8]) {
        std::fs::write(Path::new(path).join("karva-language-server"), contents).unwrap();
    }

    fn write_valid_install(
        path: &str,
        version: &str,
        asset: super::PlatformAsset,
        contents: &[u8],
    ) {
        std::fs::write(Path::new(path).join(asset.binary_name), contents).unwrap();
        write_release_marker(
            &Path::new(path).join(RELEASE_MARKER),
            version,
            asset,
            Sha256::digest(contents).into(),
        )
        .unwrap();
    }

    fn test_directory(name: &str) -> PathBuf {
        loop {
            let serial = NEXT_DIRECTORY_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos());
            let path = std::env::temp_dir().join(format!("karva-zed-{name}-{timestamp}-{serial}"));
            match std::fs::create_dir(&path) {
                Ok(()) => return path,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("failed to create test directory {path:?}: {error}"),
            }
        }
    }

    fn remove_test_directory(path: &Path) {
        std::fs::remove_dir_all(path).unwrap();
    }
}
