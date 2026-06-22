//! Windows llama.cpp sidecar binary acquisition and execution.

use std::{
    env,
    fs::{self, File},
    io::{self, Write},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use async_stream::try_stream;
use async_trait::async_trait;
use reqwest::{Client, header};
use screensearch_ports::{PortError, TextGenerator, TokenStream};
use serde::Deserialize;
use tempfile::{Builder, TempDir};
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};
use zip::ZipArchive;

use crate::{
    GENERATION_CONTEXT_TOKENS, GENERATION_DEADLINE, LlamaCppTextGenerator, MAX_GENERATED_TOKENS,
    cpu_thread_budget,
};

const RELEASES_API_URL: &str =
    "https://api.github.com/repos/ggml-org/llama.cpp/releases?per_page=12";
const RELEASE_API_PREFIX: &str = "https://api.github.com/repos/ggml-org/llama.cpp/releases/tags/";
const RELEASE_WEB_PREFIX: &str = "https://github.com/ggml-org/llama.cpp/releases/tag/";
const RELEASE_OVERRIDE_ENV: &str = "SSV2C_LLAMA_RELEASE_URL";
const CURRENT_INSTALL_DIR: &str = "current";
const LLAMA_CLI_EXE: &str = "llama-cli.exe";
const STREAM_CHUNK_BYTES: usize = 512;

/// Ensures the Windows Vulkan llama.cpp sidecar is installed and returns `llama-cli.exe`.
pub async fn ensure_binary(sidecar_root: impl AsRef<Path>) -> Result<PathBuf, PortError> {
    let sidecar_root = sidecar_root.as_ref().to_path_buf();
    if let Some(binary) = find_installed_binary(&sidecar_root)? {
        return Ok(binary);
    }
    tokio::fs::create_dir_all(&sidecar_root)
        .await
        .map_err(|error| sidecar_unavailable("create sidecar directory", error))?;
    let client = github_client()?;
    let selected = selected_release_asset(&client).await?;
    let binary = install_release_asset(&client, &sidecar_root, &selected).await?;
    info!(
        release = %selected.release_tag,
        asset = %selected.asset_name,
        "installed llama.cpp Vulkan sidecar"
    );
    Ok(binary)
}

/// Vulkan-enabled llama.cpp sidecar generator.
#[derive(Clone)]
pub struct LlamaSidecarTextGenerator {
    model_path: PathBuf,
    sidecar_root: PathBuf,
}

impl LlamaSidecarTextGenerator {
    /// Creates a generator that runs `llama-cli.exe` from the sidecar root.
    pub fn new(model_path: impl Into<PathBuf>, sidecar_root: impl Into<PathBuf>) -> Self {
        Self {
            model_path: model_path.into(),
            sidecar_root: sidecar_root.into(),
        }
    }
}

#[async_trait]
impl TextGenerator for LlamaSidecarTextGenerator {
    async fn generate(&self, prompt: String) -> Result<TokenStream, PortError> {
        if !self.model_path.is_file() {
            return Err(PortError::Unavailable(
                "local GGUF generator is not installed".to_owned(),
            ));
        }
        let binary = ensure_binary(&self.sidecar_root).await?;
        let output = run_llama_cli(&binary, &self.model_path, &prompt).await?;
        Ok(buffered_output_stream(output))
    }
}

/// Sidecar-first text generator with embedded CPU fallback.
#[derive(Clone)]
pub struct PreferredLlamaTextGenerator {
    sidecar: LlamaSidecarTextGenerator,
    cpu: LlamaCppTextGenerator,
}

impl PreferredLlamaTextGenerator {
    /// Creates a generator for one GGUF model and sidecar install root.
    pub fn new(model_path: impl Into<PathBuf>, sidecar_root: impl Into<PathBuf>) -> Self {
        let model_path = model_path.into();
        Self {
            sidecar: LlamaSidecarTextGenerator::new(&model_path, sidecar_root),
            cpu: LlamaCppTextGenerator::new(model_path),
        }
    }

    /// Returns whether the embedded CPU fallback currently has the model loaded.
    pub fn is_loaded(&self) -> bool {
        self.cpu.is_loaded()
    }

    /// Unloads the embedded CPU fallback model, if resident.
    pub fn unload(&self) -> Result<(), PortError> {
        self.cpu.unload()
    }
}

#[async_trait]
impl TextGenerator for PreferredLlamaTextGenerator {
    async fn generate(&self, prompt: String) -> Result<TokenStream, PortError> {
        if cfg!(windows) {
            match self.sidecar.generate(prompt.clone()).await {
                Ok(stream) => return Ok(stream),
                Err(error) => {
                    warn!(
                        failure_kind = sidecar_failure_kind(&error),
                        "llama.cpp Vulkan sidecar failed; falling back to embedded CPU provider"
                    );
                }
            }
        }
        self.cpu.generate(prompt).await
    }
}

#[derive(Clone, Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    draft: bool,
    assets: Vec<GitHubAsset>,
}

#[derive(Clone, Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SelectedReleaseAsset {
    release_tag: String,
    asset_name: String,
    download_url: String,
}

fn github_client() -> Result<Client, PortError> {
    Client::builder()
        .user_agent(format!("ScreenSearchV2/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| sidecar_unavailable("create GitHub client", error))
}

async fn selected_release_asset(client: &Client) -> Result<SelectedReleaseAsset, PortError> {
    if let Some(override_url) = env::var_os(RELEASE_OVERRIDE_ENV) {
        let override_url = override_url.to_string_lossy();
        let api_url = release_api_url_from_override(&override_url)?;
        let release: GitHubRelease = get_json(client, &api_url).await?;
        return select_release_asset(&[release]).ok_or_else(|| {
            PortError::Unavailable(
                "configured llama.cpp release has no Windows Vulkan x64 asset".to_owned(),
            )
        });
    }

    let releases: Vec<GitHubRelease> = get_json(client, RELEASES_API_URL).await?;
    select_release_asset(&releases).ok_or_else(|| {
        PortError::Unavailable("no recent llama.cpp Windows Vulkan x64 asset found".to_owned())
    })
}

async fn get_json<T: serde::de::DeserializeOwned>(
    client: &Client,
    url: &str,
) -> Result<T, PortError> {
    client
        .get(url)
        .send()
        .await
        .map_err(|error| sidecar_unavailable("fetch llama.cpp release metadata", error))?
        .error_for_status()
        .map_err(|error| sidecar_unavailable("fetch llama.cpp release metadata", error))?
        .text()
        .await
        .map_err(|error| sidecar_unavailable("read llama.cpp release metadata", error))
        .and_then(|body| {
            serde_json::from_str(&body)
                .map_err(|error| sidecar_unavailable("parse llama.cpp release metadata", error))
        })
}

fn select_release_asset(releases: &[GitHubRelease]) -> Option<SelectedReleaseAsset> {
    releases
        .iter()
        .filter(|release| !release.draft)
        .find_map(|release| {
            release
                .assets
                .iter()
                .find(|asset| asset_matches_windows_vulkan(&asset.name))
                .map(|asset| SelectedReleaseAsset {
                    release_tag: release.tag_name.clone(),
                    asset_name: asset.name.clone(),
                    download_url: asset.browser_download_url.clone(),
                })
        })
}

fn asset_matches_windows_vulkan(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("llama-") && name.ends_with("-bin-win-vulkan-x64.zip")
}

fn release_api_url_from_override(value: &str) -> Result<String, PortError> {
    let value = value
        .trim()
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .trim_end_matches('/');
    if value.is_empty() {
        return Err(PortError::InvalidData(
            "llama sidecar release URL is empty".to_owned(),
        ));
    }
    if value.starts_with(RELEASE_API_PREFIX) {
        return Ok(value.to_owned());
    }
    if let Some(tag) = value.strip_prefix(RELEASE_WEB_PREFIX)
        && !tag.is_empty()
        && !tag.contains('/')
    {
        return Ok(format!("{RELEASE_API_PREFIX}{tag}"));
    }
    Err(PortError::InvalidData(
        "llama sidecar release URL must be a ggml-org/llama.cpp GitHub release".to_owned(),
    ))
}

async fn install_release_asset(
    client: &Client,
    sidecar_root: &Path,
    selected: &SelectedReleaseAsset,
) -> Result<PathBuf, PortError> {
    tokio::fs::create_dir_all(sidecar_root)
        .await
        .map_err(|error| sidecar_unavailable("create sidecar directory", error))?;
    let download = Builder::new()
        .prefix("download-")
        .suffix(".zip")
        .tempfile_in(sidecar_root)
        .map_err(|error| sidecar_unavailable("create sidecar download file", error))?;
    download_asset(client, &selected.download_url, download.path()).await?;
    let staging = Builder::new()
        .prefix("staging-")
        .tempdir_in(sidecar_root)
        .map_err(|error| sidecar_unavailable("create sidecar staging directory", error))?;
    extract_zip_safely(download.path(), staging.path())?;
    swap_staging_into_current(sidecar_root, staging)
}

async fn download_asset(client: &Client, url: &str, destination: &Path) -> Result<(), PortError> {
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|error| sidecar_unavailable("download llama.cpp sidecar", error))?
        .error_for_status()
        .map_err(|error| sidecar_unavailable("download llama.cpp sidecar", error))?;
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if content_type.starts_with("text/") {
        return Err(PortError::Unavailable(
            "llama sidecar download returned text instead of a zip archive".to_owned(),
        ));
    }
    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(destination)
        .await
        .map_err(|error| sidecar_unavailable("open sidecar download file", error))?;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| sidecar_unavailable("download llama.cpp sidecar", error))?
    {
        output
            .write_all(&chunk)
            .await
            .map_err(|error| sidecar_unavailable("write sidecar download file", error))?;
    }
    output
        .flush()
        .await
        .map_err(|error| sidecar_unavailable("flush sidecar download file", error))?;
    Ok(())
}

fn extract_zip_safely(archive_path: &Path, staging: &Path) -> Result<PathBuf, PortError> {
    fs::create_dir_all(staging)
        .map_err(|error| sidecar_unavailable("create sidecar staging directory", error))?;
    let file = File::open(archive_path)
        .map_err(|error| sidecar_unavailable("open sidecar zip archive", error))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| sidecar_unavailable("read sidecar zip archive", error))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| sidecar_unavailable("read sidecar zip entry", error))?;
        let relative_path = safe_zip_entry_path(entry.name())?;
        let output_path = staging.join(relative_path);
        if entry.is_dir() {
            fs::create_dir_all(&output_path)
                .map_err(|error| sidecar_unavailable("create sidecar directory", error))?;
            continue;
        }
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| sidecar_unavailable("create sidecar directory", error))?;
        }
        let mut output = File::create(&output_path)
            .map_err(|error| sidecar_unavailable("create sidecar file", error))?;
        io::copy(&mut entry, &mut output)
            .map_err(|error| sidecar_unavailable("extract sidecar file", error))?;
    }
    find_llama_cli_under(staging)?.ok_or_else(|| {
        PortError::Unavailable("llama sidecar archive did not contain llama-cli.exe".to_owned())
    })
}

fn safe_zip_entry_path(name: &str) -> Result<PathBuf, PortError> {
    if name.trim().is_empty() {
        return Err(PortError::InvalidData(
            "unsafe zip entry path in llama sidecar archive".to_owned(),
        ));
    }
    let path = Path::new(name);
    let mut safe = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => safe.push(value),
            Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
                return Err(PortError::InvalidData(
                    "unsafe zip entry path in llama sidecar archive".to_owned(),
                ));
            }
        }
    }
    if safe.as_os_str().is_empty() {
        return Err(PortError::InvalidData(
            "unsafe zip entry path in llama sidecar archive".to_owned(),
        ));
    }
    Ok(safe)
}

fn swap_staging_into_current(sidecar_root: &Path, staging: TempDir) -> Result<PathBuf, PortError> {
    let staging_path = staging.keep();
    let current = sidecar_root.join(CURRENT_INSTALL_DIR);
    let previous = sidecar_root.join(format!("previous-{}", unique_suffix()));
    if previous.exists() {
        fs::remove_dir_all(&previous)
            .map_err(|error| sidecar_unavailable("remove old sidecar backup", error))?;
    }
    let had_current = current.exists();
    if had_current {
        fs::rename(&current, &previous)
            .map_err(|error| sidecar_unavailable("stage existing sidecar install", error))?;
    }
    if let Err(error) = fs::rename(&staging_path, &current) {
        if had_current {
            let _ = fs::rename(&previous, &current);
        }
        let _ = fs::remove_dir_all(&staging_path);
        return Err(sidecar_unavailable(
            "install sidecar staging directory",
            error,
        ));
    }
    if previous.exists()
        && let Err(error) = fs::remove_dir_all(&previous)
    {
        warn!(
            failure_kind = io_error_kind(&error),
            "could not remove previous llama sidecar install"
        );
    }
    find_llama_cli_under(&current)?.ok_or_else(|| {
        PortError::Unavailable("llama sidecar archive did not contain llama-cli.exe".to_owned())
    })
}

fn find_installed_binary(sidecar_root: &Path) -> Result<Option<PathBuf>, PortError> {
    let current = sidecar_root.join(CURRENT_INSTALL_DIR);
    if !current.is_dir() {
        return Ok(None);
    }
    find_llama_cli_under(&current)
}

fn find_llama_cli_under(root: &Path) -> Result<Option<PathBuf>, PortError> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(sidecar_unavailable("read sidecar directory", error)),
        };
        for entry in entries {
            let entry = entry.map_err(|error| sidecar_unavailable("read sidecar file", error))?;
            let path = entry.path();
            let metadata = entry
                .metadata()
                .map_err(|error| sidecar_unavailable("read sidecar file metadata", error))?;
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file()
                && path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|name| name.eq_ignore_ascii_case(LLAMA_CLI_EXE))
            {
                return Ok(Some(path));
            }
        }
    }
    Ok(None)
}

async fn run_llama_cli(
    binary: &Path,
    model_path: &Path,
    prompt: &str,
) -> Result<String, PortError> {
    let mut prompt_file = Builder::new()
        .prefix("screensearch-llama-prompt-")
        .suffix(".txt")
        .tempfile()
        .map_err(|error| sidecar_unavailable("create sidecar prompt file", error))?;
    prompt_file
        .write_all(prompt.as_bytes())
        .map_err(|error| sidecar_unavailable("write sidecar prompt file", error))?;
    prompt_file
        .flush()
        .map_err(|error| sidecar_unavailable("flush sidecar prompt file", error))?;
    let threads = cpu_thread_budget().to_string();
    let context_tokens = GENERATION_CONTEXT_TOKENS.to_string();
    let token_cap = MAX_GENERATED_TOKENS.to_string();
    let mut command = tokio::process::Command::new(binary);
    command
        .arg("--model")
        .arg(model_path)
        .arg("--file")
        .arg(prompt_file.path())
        .arg("--ctx-size")
        .arg(context_tokens)
        .arg("--threads")
        .arg(&threads)
        .arg("--threads-batch")
        .arg(threads)
        .arg("--gpu-layers")
        .arg("all")
        .arg("--predict")
        .arg(token_cap)
        .arg("--conversation")
        .arg("--single-turn")
        .arg("--no-display-prompt")
        .arg("--no-show-timings")
        .arg("--color")
        .arg("off")
        .arg("--log-disable")
        .kill_on_drop(true);
    if let Some(directory) = binary.parent() {
        command.current_dir(directory);
    }
    let output = tokio::time::timeout(GENERATION_DEADLINE, command.output())
        .await
        .map_err(|_| PortError::Transient("llama sidecar generation timed out".to_owned()))?
        .map_err(|error| sidecar_unavailable("run llama sidecar", error))?;
    if !output.status.success() {
        return Err(PortError::Unavailable(format!(
            "llama sidecar exited with status {}",
            output
                .status
                .code()
                .map_or_else(|| "terminated".to_owned(), |code| code.to_string())
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn buffered_output_stream(output: String) -> TokenStream {
    Box::pin(try_stream! {
        let mut chunk = String::new();
        for character in output.chars() {
            chunk.push(character);
            if chunk.len() >= STREAM_CHUNK_BYTES {
                yield std::mem::take(&mut chunk);
            }
        }
        if !chunk.is_empty() {
            yield chunk;
        }
    })
}

fn unique_suffix() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    format!("{}-{millis}", std::process::id())
}

fn sidecar_failure_kind(error: &PortError) -> &'static str {
    match error {
        PortError::Unavailable(_) => "unavailable",
        PortError::InvalidData(_) => "invalid_data",
        PortError::Transient(_) => "transient",
        PortError::Denied(_) => "denied",
        PortError::Automation(_) => "automation",
        PortError::Internal(_) => "internal",
    }
}

fn sidecar_unavailable(context: &str, error: impl std::fmt::Display) -> PortError {
    PortError::Unavailable(format!("{context}: {error}"))
}

fn io_error_kind(error: &io::Error) -> &'static str {
    match error.kind() {
        io::ErrorKind::NotFound => "not_found",
        io::ErrorKind::PermissionDenied => "permission_denied",
        io::ErrorKind::AlreadyExists => "already_exists",
        io::ErrorKind::InvalidInput => "invalid_input",
        io::ErrorKind::TimedOut => "timed_out",
        _ => "io_error",
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Write, path::Path};

    use tempfile::TempDir;
    use zip::{
        CompressionMethod, ZipWriter,
        write::{FileOptions, SimpleFileOptions},
    };

    use super::{
        GitHubAsset, GitHubRelease, asset_matches_windows_vulkan, extract_zip_safely,
        release_api_url_from_override, select_release_asset,
    };

    fn release(tag: &str, draft: bool, assets: &[&str]) -> GitHubRelease {
        GitHubRelease {
            tag_name: tag.to_owned(),
            draft,
            assets: assets
                .iter()
                .map(|name| GitHubAsset {
                    name: (*name).to_owned(),
                    browser_download_url: format!("https://example.test/{name}"),
                })
                .collect(),
        }
    }

    #[test]
    fn asset_matcher_accepts_windows_vulkan_x64_zip() {
        assert!(asset_matches_windows_vulkan(
            "llama-b9758-bin-win-vulkan-x64.zip"
        ));
    }

    #[test]
    fn asset_matcher_rejects_non_vulkan_windows_x64_assets() {
        for name in [
            "llama-b9758-bin-win-cpu-x64.zip",
            "llama-b9758-bin-win-cuda-12.4-x64.zip",
            "llama-b9758-bin-ubuntu-vulkan-x64.tar.gz",
            "llama-b9758-bin-win-vulkan-arm64.zip",
            "cudart-llama-bin-win-cuda-12.4-x64.zip",
        ] {
            assert!(
                !asset_matches_windows_vulkan(name),
                "{name} should not match the Windows Vulkan sidecar asset"
            );
        }
    }

    #[test]
    fn release_selection_skips_incomplete_newest_release() {
        let releases = vec![
            release("b9759", false, &["cudart-llama-bin-win-cuda-12.4-x64.zip"]),
            release(
                "b9758",
                false,
                &[
                    "llama-b9758-bin-win-cpu-x64.zip",
                    "llama-b9758-bin-win-vulkan-x64.zip",
                ],
            ),
        ];

        let selected = select_release_asset(&releases).expect("asset should be selected");

        assert_eq!(selected.release_tag, "b9758");
        assert_eq!(selected.asset_name, "llama-b9758-bin-win-vulkan-x64.zip");
        assert_eq!(
            selected.download_url,
            "https://example.test/llama-b9758-bin-win-vulkan-x64.zip"
        );
    }

    #[test]
    fn override_release_url_accepts_github_release_forms() {
        assert_eq!(
            release_api_url_from_override(
                "https://github.com/ggml-org/llama.cpp/releases/tag/b9758"
            )
            .unwrap(),
            "https://api.github.com/repos/ggml-org/llama.cpp/releases/tags/b9758"
        );
        assert_eq!(
            release_api_url_from_override(
                "https://api.github.com/repos/ggml-org/llama.cpp/releases/tags/b9758"
            )
            .unwrap(),
            "https://api.github.com/repos/ggml-org/llama.cpp/releases/tags/b9758"
        );
    }

    #[test]
    fn safe_zip_extraction_accepts_cli_binary() {
        let directory = TempDir::new().unwrap();
        let archive = directory.path().join("llama.zip");
        write_zip(
            &archive,
            &[("llama-b9758-bin-win-vulkan-x64/llama-cli.exe", b"binary")],
        );
        let staging = directory.path().join("staging");

        let binary = extract_zip_safely(&archive, &staging).unwrap();

        assert_eq!(
            binary,
            staging.join("llama-b9758-bin-win-vulkan-x64/llama-cli.exe")
        );
        assert_eq!(std::fs::read(binary).unwrap(), b"binary");
    }

    #[test]
    fn safe_zip_extraction_rejects_path_traversal() {
        let directory = TempDir::new().unwrap();
        let archive = directory.path().join("llama.zip");
        write_zip(&archive, &[("../llama-cli.exe", b"escape")]);
        let staging = directory.path().join("staging");

        let error = extract_zip_safely(&archive, &staging).unwrap_err();

        assert!(error.to_string().contains("unsafe zip entry"));
        assert!(!directory.path().join("llama-cli.exe").exists());
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options: SimpleFileOptions =
            FileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, contents) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(contents).unwrap();
        }
        zip.finish().unwrap();
    }
}
