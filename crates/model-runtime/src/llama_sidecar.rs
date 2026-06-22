//! Windows llama.cpp sidecar binary acquisition and execution.

use std::{
    env,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    process::Stdio,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use async_stream::try_stream;
use async_trait::async_trait;
use encoding_rs::CoderResult;
use futures::StreamExt;
use reqwest::{Client, header};
use screensearch_ports::{PortError, TextGenerator, TokenStream};
use serde::{Deserialize, Serialize};
use tempfile::{Builder, NamedTempFile, TempDir};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time::{Instant as TokioInstant, timeout, timeout_at},
};
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
const INSTALL_METADATA_FILE: &str = "screensearch-sidecar.json";
const LLAMA_CLI_EXE: &str = "llama-cli.exe";
const STDOUT_READ_BYTES: usize = 512;
const STDOUT_STREAM_CHUNK_BYTES: usize = 512;
const MAX_SIDECAR_STDOUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_DOWNLOAD_BYTES: u64 = 250 * 1024 * 1024;
const MAX_EXTRACTED_BYTES: u64 = 500 * 1024 * 1024;
const METADATA_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);
const SIDECAR_EXIT_TRAILER: &str = "Exiting...";
const SIDECAR_EXIT_TRAILER_HOLDBACK_BYTES: usize = SIDECAR_EXIT_TRAILER.len() + 8;

static SIDECAR_INSTALL_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Ensures the Windows Vulkan llama.cpp sidecar is installed and returns `llama-cli.exe`.
pub async fn ensure_binary(sidecar_root: impl AsRef<Path>) -> Result<PathBuf, PortError> {
    let sidecar_root = sidecar_root.as_ref().to_path_buf();
    let required_release_tag = release_tag_from_override_env()?;
    if let Some(binary) =
        find_installed_binary_for_release(&sidecar_root, required_release_tag.as_deref())?
    {
        return Ok(binary);
    }
    let _install_guard = SIDECAR_INSTALL_LOCK.lock().await;
    if let Some(binary) =
        find_installed_binary_for_release(&sidecar_root, required_release_tag.as_deref())?
    {
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
        run_llama_cli(&binary, &self.model_path, &prompt)
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
                Ok(stream) => {
                    return Ok(sidecar_stream_with_cpu_fallback(
                        stream,
                        self.cpu.clone(),
                        prompt,
                    ));
                }
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
    prerelease: bool,
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

#[derive(Clone, Debug, Deserialize, Serialize)]
struct InstalledReleaseMetadata {
    release_tag: String,
    asset_name: String,
}

fn github_client() -> Result<Client, PortError> {
    Client::builder()
        .user_agent(format!("ScreenSearchV2/{}", env!("CARGO_PKG_VERSION")))
        .connect_timeout(CONNECT_TIMEOUT)
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
    let response = timeout(METADATA_TIMEOUT, client.get(url).send())
        .await
        .map_err(|_| sidecar_timeout("fetch llama.cpp release metadata"))?
        .map_err(|error| sidecar_unavailable("fetch llama.cpp release metadata", error))?;
    let response = response
        .error_for_status()
        .map_err(|error| sidecar_unavailable("fetch llama.cpp release metadata", error))?;
    let body = timeout(METADATA_TIMEOUT, response.text())
        .await
        .map_err(|_| sidecar_timeout("read llama.cpp release metadata"))?
        .map_err(|error| sidecar_unavailable("read llama.cpp release metadata", error))?;
    serde_json::from_str(&body)
        .map_err(|error| sidecar_unavailable("parse llama.cpp release metadata", error))
}

fn select_release_asset(releases: &[GitHubRelease]) -> Option<SelectedReleaseAsset> {
    releases
        .iter()
        .filter(|release| !release.draft && !release.prerelease)
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
    let tag = release_tag_from_override(value)?;
    Ok(format!("{RELEASE_API_PREFIX}{tag}"))
}

fn release_tag_from_override_env() -> Result<Option<String>, PortError> {
    env::var_os(RELEASE_OVERRIDE_ENV)
        .map(|override_url| release_tag_from_override(&override_url.to_string_lossy()))
        .transpose()
}

fn release_tag_from_override(value: &str) -> Result<String, PortError> {
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
    match value.strip_prefix(RELEASE_API_PREFIX) {
        Some(tag) if valid_release_tag(tag) => return Ok(tag.to_owned()),
        _ => {}
    }
    match value.strip_prefix(RELEASE_WEB_PREFIX) {
        Some(tag) if valid_release_tag(tag) => return Ok(tag.to_owned()),
        _ => {}
    }
    Err(PortError::InvalidData(
        "llama sidecar release URL must be a ggml-org/llama.cpp GitHub release".to_owned(),
    ))
}

fn valid_release_tag(tag: &str) -> bool {
    !tag.is_empty() && !tag.contains('/')
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
    let download_path = download.path().to_path_buf();
    let staging_path = staging.path().to_path_buf();
    let sidecar_root = sidecar_root.to_path_buf();
    let selected = selected.clone();
    tokio::task::spawn_blocking(move || {
        extract_zip_safely(&download_path, &staging_path)?;
        write_install_metadata(&staging_path, &selected.release_tag, &selected.asset_name)?;
        swap_staging_into_current(&sidecar_root, staging)
    })
    .await
    .map_err(|error| PortError::Internal(format!("sidecar install task failed: {error}")))?
}

async fn download_asset(client: &Client, url: &str, destination: &Path) -> Result<(), PortError> {
    let deadline = TokioInstant::now() + DOWNLOAD_TIMEOUT;
    let mut response = timeout_at(deadline, client.get(url).send())
        .await
        .map_err(|_| sidecar_timeout("download llama.cpp sidecar"))?
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
    let content_length = response.content_length().unwrap_or(0);
    if content_length > MAX_DOWNLOAD_BYTES {
        return Err(PortError::Unavailable(
            "llama sidecar download exceeded size limit".to_owned(),
        ));
    }
    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(destination)
        .await
        .map_err(|error| sidecar_unavailable("open sidecar download file", error))?;
    let mut downloaded_bytes = 0_u64;
    while let Some(chunk) = timeout_at(deadline, response.chunk())
        .await
        .map_err(|_| sidecar_timeout("download llama.cpp sidecar"))?
        .map_err(|error| sidecar_unavailable("download llama.cpp sidecar", error))?
    {
        downloaded_bytes =
            checked_sidecar_download_size(downloaded_bytes, chunk.len(), MAX_DOWNLOAD_BYTES)?;
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

fn checked_sidecar_download_size(
    downloaded_bytes: u64,
    chunk_bytes: usize,
    maximum_download_bytes: u64,
) -> Result<u64, PortError> {
    let chunk_bytes = u64::try_from(chunk_bytes)
        .map_err(|_| PortError::Internal("sidecar download chunk is too large".to_owned()))?;
    let total = downloaded_bytes.saturating_add(chunk_bytes);
    if total > maximum_download_bytes {
        return Err(PortError::Unavailable(
            "llama sidecar download exceeded size limit".to_owned(),
        ));
    }
    Ok(total)
}

fn extract_zip_safely(archive_path: &Path, staging: &Path) -> Result<PathBuf, PortError> {
    extract_zip_safely_with_limit(archive_path, staging, MAX_EXTRACTED_BYTES)
}

fn extract_zip_safely_with_limit(
    archive_path: &Path,
    staging: &Path,
    maximum_extracted_bytes: u64,
) -> Result<PathBuf, PortError> {
    fs::create_dir_all(staging)
        .map_err(|error| sidecar_unavailable("create sidecar staging directory", error))?;
    let file = File::open(archive_path)
        .map_err(|error| sidecar_unavailable("open sidecar zip archive", error))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| sidecar_unavailable("read sidecar zip archive", error))?;
    let mut extracted_bytes = 0_u64;
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
        if entry.size() > maximum_extracted_bytes.saturating_sub(extracted_bytes) {
            return Err(PortError::InvalidData(
                "llama sidecar archive exceeds extraction size limit".to_owned(),
            ));
        }
        let copied = copy_zip_entry_with_limit(
            &mut entry,
            &mut output,
            maximum_extracted_bytes.saturating_sub(extracted_bytes),
        )?;
        extracted_bytes = extracted_bytes.saturating_add(copied);
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

fn copy_zip_entry_with_limit(
    input: &mut impl Read,
    output: &mut File,
    remaining_bytes: u64,
) -> Result<u64, PortError> {
    let mut copied = 0_u64;
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = input
            .read(&mut buffer)
            .map_err(|error| sidecar_unavailable("extract sidecar file", error))?;
        if read == 0 {
            return Ok(copied);
        }
        copied = copied.saturating_add(read as u64);
        if copied > remaining_bytes {
            return Err(PortError::InvalidData(
                "llama sidecar archive exceeds extraction size limit".to_owned(),
            ));
        }
        output
            .write_all(&buffer[..read])
            .map_err(|error| sidecar_unavailable("extract sidecar file", error))?;
    }
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
    remove_previous_install_backup(&previous);
    find_llama_cli_under(&current)?.ok_or_else(|| {
        PortError::Unavailable("llama sidecar archive did not contain llama-cli.exe".to_owned())
    })
}

fn remove_previous_install_backup(previous: &Path) {
    if !previous.exists() {
        return;
    }
    if let Err(error) = fs::remove_dir_all(previous) {
        warn!(
            failure_kind = io_error_kind(&error),
            "could not remove previous llama sidecar install"
        );
    }
}

fn write_install_metadata(
    install_root: &Path,
    release_tag: &str,
    asset_name: &str,
) -> Result<(), PortError> {
    let metadata = InstalledReleaseMetadata {
        release_tag: release_tag.to_owned(),
        asset_name: asset_name.to_owned(),
    };
    let payload = serde_json::to_vec_pretty(&metadata)
        .map_err(|error| PortError::Internal(format!("serialize sidecar metadata: {error}")))?;
    fs::write(install_root.join(INSTALL_METADATA_FILE), payload)
        .map_err(|error| sidecar_unavailable("write sidecar metadata", error))
}

fn read_install_metadata(
    install_root: &Path,
) -> Result<Option<InstalledReleaseMetadata>, PortError> {
    let path = install_root.join(INSTALL_METADATA_FILE);
    let payload = match fs::read(&path) {
        Ok(payload) => payload,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(sidecar_unavailable("read sidecar metadata", error)),
    };
    match serde_json::from_slice(&payload) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) => {
            warn!(
                failure_kind = "invalid_metadata",
                %error,
                "could not parse llama sidecar metadata"
            );
            Ok(None)
        }
    }
}

fn find_installed_binary_for_release(
    sidecar_root: &Path,
    required_release_tag: Option<&str>,
) -> Result<Option<PathBuf>, PortError> {
    let Some(binary) = find_installed_binary(sidecar_root)? else {
        return Ok(None);
    };
    let Some(required_release_tag) = required_release_tag else {
        return Ok(Some(binary));
    };
    let current = sidecar_root.join(CURRENT_INSTALL_DIR);
    let Some(metadata) = read_install_metadata(&current)? else {
        return Ok(None);
    };
    if metadata.release_tag == required_release_tag {
        Ok(Some(binary))
    } else {
        Ok(None)
    }
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

fn run_llama_cli(binary: &Path, model_path: &Path, prompt: &str) -> Result<TokenStream, PortError> {
    let prompt_file = write_prompt_tempfile(prompt)?;
    let threads = cpu_thread_budget().to_string();
    let context_tokens = GENERATION_CONTEXT_TOKENS.to_string();
    let token_cap = MAX_GENERATED_TOKENS.to_string();
    let args = build_llama_cli_args(
        model_path,
        prompt_file.path(),
        &threads,
        &context_tokens,
        &token_cap,
    );
    let mut command = tokio::process::Command::new(binary);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if let Some(directory) = binary.parent() {
        command.current_dir(directory);
    }
    let mut child = command
        .spawn()
        .map_err(|error| sidecar_unavailable("run llama sidecar", error))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| PortError::Internal("llama sidecar stdout was not piped".to_owned()))?;
    info!(
        threads = %threads,
        context_tokens = %context_tokens,
        token_cap = %token_cap,
        "starting llama.cpp Vulkan sidecar generation"
    );
    let prompt_text = prompt.to_owned();
    Ok(Box::pin(try_stream! {
        let _prompt_file = prompt_file;
        let mut stdout = stdout;
        let mut buffer = [0_u8; STDOUT_READ_BYTES];
        let mut stdout_bytes = 0_usize;
        let mut answer_bytes = 0_usize;
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut sanitizer = SidecarStdoutSanitizer::new(&prompt_text);
        let deadline = TokioInstant::now() + GENERATION_DEADLINE;
        let started = Instant::now();
        loop {
            let read =
                read_sidecar_stdout(&mut stdout, &mut child, &mut buffer, deadline).await?;
            if read == 0 {
                break;
            }
            if stdout_bytes.saturating_add(read) > MAX_SIDECAR_STDOUT_BYTES {
                Err(PortError::Unavailable(
                    "llama sidecar stdout exceeded the capture limit".to_owned(),
                ))?;
            }
            stdout_bytes = stdout_bytes.saturating_add(read);
            let text = decode_sidecar_stdout(&mut decoder, &buffer[..read], false)?;
            let answer = sanitizer.push(&text);
            if !answer.is_empty() {
                answer_bytes = answer_bytes.saturating_add(answer.len());
                yield answer;
            }
        }
        let trailing = decode_sidecar_stdout(&mut decoder, &[], true)?;
        let answer = sanitizer.push(&trailing);
        if !answer.is_empty() {
            answer_bytes = answer_bytes.saturating_add(answer.len());
            yield answer;
        }
        let status = wait_for_sidecar_exit(&mut child, deadline).await?;
        if !status.success() {
            Err(PortError::Unavailable(format!(
                "llama sidecar exited with status {}",
                status
                    .code()
                    .map_or_else(|| "terminated".to_owned(), |code| code.to_string())
            )))?;
        }
        let answer = sanitizer.finish()?;
        if answer.is_empty() {
            if answer_bytes == 0 {
                Err(PortError::Unavailable(
                    "llama sidecar returned no answer text".to_owned(),
                ))?;
            }
        } else {
            answer_bytes = answer_bytes.saturating_add(answer.len());
            yield answer;
        }
        info!(
            elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            stdout_bytes,
            answer_bytes,
            "completed llama.cpp Vulkan sidecar generation"
        );
    }))
}

fn write_prompt_tempfile(prompt: &str) -> Result<NamedTempFile, PortError> {
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
    Ok(prompt_file)
}

fn build_llama_cli_args(
    model_path: &Path,
    prompt_path: &Path,
    threads: &str,
    context_tokens: &str,
    token_cap: &str,
) -> Vec<String> {
    vec![
        "--model".to_owned(),
        model_path.display().to_string(),
        "--file".to_owned(),
        prompt_path.display().to_string(),
        "--ctx-size".to_owned(),
        context_tokens.to_owned(),
        "--threads".to_owned(),
        threads.to_owned(),
        "--threads-batch".to_owned(),
        threads.to_owned(),
        "--gpu-layers".to_owned(),
        "all".to_owned(),
        "--predict".to_owned(),
        token_cap.to_owned(),
        "--temp".to_owned(),
        "0".to_owned(),
        "--seed".to_owned(),
        "0".to_owned(),
        // Together these flags make chat-template models produce one assistant turn and exit.
        // If upstream behavior regresses, the generation deadline still kills the sidecar.
        "--conversation".to_owned(),
        "--single-turn".to_owned(),
        "--no-display-prompt".to_owned(),
        "--no-show-timings".to_owned(),
        "--color".to_owned(),
        "off".to_owned(),
        "--log-disable".to_owned(),
        "--simple-io".to_owned(),
        "--reasoning".to_owned(),
        "off".to_owned(),
    ]
}

fn decode_sidecar_stdout(
    decoder: &mut encoding_rs::Decoder,
    bytes: &[u8],
    last: bool,
) -> Result<String, PortError> {
    let capacity = decoder
        .max_utf8_buffer_length(bytes.len())
        .ok_or_else(|| PortError::Internal("sidecar stdout chunk is too large".to_owned()))?;
    let mut text = String::with_capacity(capacity);
    let (result, _, _) = decoder.decode_to_string(bytes, &mut text, last);
    if result == CoderResult::OutputFull {
        return Err(PortError::Internal(
            "sidecar stdout decoder output buffer was exhausted".to_owned(),
        ));
    }
    Ok(text)
}

struct SidecarStdoutSanitizer {
    prompt: String,
    pending: String,
    mode: SidecarStdoutMode,
    emitted_text: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SidecarStdoutMode {
    Undecided,
    AwaitingPromptBoundary,
    Streaming,
}

impl SidecarStdoutSanitizer {
    fn new(prompt: &str) -> Self {
        Self {
            prompt: normalize_newlines(prompt),
            pending: String::new(),
            mode: SidecarStdoutMode::Undecided,
            emitted_text: false,
        }
    }

    fn push(&mut self, text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }
        self.pending.push_str(&normalize_newlines(text));
        match self.mode {
            SidecarStdoutMode::Undecided => self.process_undecided(),
            SidecarStdoutMode::AwaitingPromptBoundary => self.process_prompt_boundary(),
            SidecarStdoutMode::Streaming => self.flush_streaming(),
        }
    }

    fn finish(&mut self) -> Result<String, PortError> {
        match self.mode {
            SidecarStdoutMode::Undecided => {
                if self.pending.trim().is_empty() {
                    Ok(String::new())
                } else if looks_like_llama_cli_transcript_prefix(&self.pending) {
                    Err(ambiguous_sidecar_transcript())
                } else {
                    self.mode = SidecarStdoutMode::Streaming;
                    Ok(self.finish_streaming())
                }
            }
            SidecarStdoutMode::AwaitingPromptBoundary => Err(ambiguous_sidecar_transcript()),
            SidecarStdoutMode::Streaming => Ok(self.finish_streaming()),
        }
    }

    fn process_undecided(&mut self) -> String {
        if let Some(output) = self.discard_through_prompt_boundary() {
            return output;
        }
        if looks_like_llama_cli_transcript_prefix(&self.pending) {
            self.mode = SidecarStdoutMode::AwaitingPromptBoundary;
            return String::new();
        }
        if is_possible_llama_cli_transcript_prefix(&self.pending) {
            return String::new();
        }
        self.mode = SidecarStdoutMode::Streaming;
        self.flush_streaming()
    }

    fn process_prompt_boundary(&mut self) -> String {
        self.discard_through_prompt_boundary().unwrap_or_default()
    }

    fn discard_through_prompt_boundary(&mut self) -> Option<String> {
        let prompt_end = self
            .pending
            .find(&self.prompt)
            .map(|offset| offset + self.prompt.len())?;
        self.pending.drain(..prompt_end);
        self.mode = SidecarStdoutMode::Streaming;
        Some(self.flush_streaming())
    }

    fn flush_streaming(&mut self) -> String {
        self.trim_leading_before_first_emit();
        let holdback_bytes = SIDECAR_EXIT_TRAILER_HOLDBACK_BYTES;
        let target = self
            .pending
            .len()
            .saturating_sub(holdback_bytes)
            .min(STDOUT_STREAM_CHUNK_BYTES);
        let emit_until = utf8_floor_boundary(&self.pending, target);
        if emit_until == 0 {
            return String::new();
        }
        let output = self.pending[..emit_until].to_owned();
        self.pending.drain(..emit_until);
        if !output.is_empty() {
            self.emitted_text = true;
        }
        output
    }

    fn finish_streaming(&mut self) -> String {
        self.trim_leading_before_first_emit();
        let output = strip_llama_cli_trailer(&self.pending).trim_end().to_owned();
        self.pending.clear();
        if !output.is_empty() {
            self.emitted_text = true;
        }
        output
    }

    fn trim_leading_before_first_emit(&mut self) {
        if self.emitted_text {
            return;
        }
        let trimmed = self.pending.trim_start();
        if trimmed.len() != self.pending.len() {
            self.pending = trimmed.to_owned();
        }
    }
}

fn ambiguous_sidecar_transcript() -> PortError {
    PortError::InvalidData(
        "llama sidecar emitted an interactive transcript without a prompt boundary".to_owned(),
    )
}

fn utf8_floor_boundary(text: &str, target: usize) -> usize {
    if target >= text.len() {
        return text.len();
    }
    text.char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= target)
        .last()
        .unwrap_or(0)
}

#[cfg(test)]
fn sanitize_sidecar_stdout(stdout: &str, prompt: &str) -> Result<String, PortError> {
    let normalized_stdout = normalize_newlines(stdout);
    let normalized_prompt = normalize_newlines(prompt);
    let body = if looks_like_llama_cli_transcript(&normalized_stdout) {
        let prompt_end = normalized_stdout
            .find(&normalized_prompt)
            .map(|offset| offset + normalized_prompt.len())
            .ok_or_else(ambiguous_sidecar_transcript)?;
        &normalized_stdout[prompt_end..]
    } else {
        normalized_stdout.as_str()
    };
    Ok(strip_llama_cli_trailer(body).trim().to_owned())
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

#[cfg(test)]
fn looks_like_llama_cli_transcript(text: &str) -> bool {
    looks_like_llama_cli_transcript_prefix(text)
}

fn looks_like_llama_cli_transcript_prefix(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with("Loading model")
        || text.contains("available commands:")
        || text.lines().any(|line| line.starts_with("> "))
}

fn is_possible_llama_cli_transcript_prefix(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.is_empty() || "Loading model".starts_with(trimmed) || "> ".starts_with(trimmed)
}

fn strip_llama_cli_trailer(text: &str) -> &str {
    let trimmed = text.trim();
    trimmed
        .strip_suffix(SIDECAR_EXIT_TRAILER)
        .map_or(trimmed, str::trim)
}

async fn read_sidecar_stdout(
    stdout: &mut tokio::process::ChildStdout,
    child: &mut tokio::process::Child,
    buffer: &mut [u8],
    deadline: TokioInstant,
) -> Result<usize, PortError> {
    match timeout_at(deadline, stdout.read(buffer)).await {
        Ok(Ok(read)) => Ok(read),
        Ok(Err(error)) => Err(sidecar_unavailable("read llama sidecar stdout", error)),
        Err(_) => Err(sidecar_generation_timeout(child).await),
    }
}

async fn wait_for_sidecar_exit(
    child: &mut tokio::process::Child,
    deadline: TokioInstant,
) -> Result<std::process::ExitStatus, PortError> {
    match timeout_at(deadline, child.wait()).await {
        Ok(Ok(status)) => Ok(status),
        Ok(Err(error)) => Err(sidecar_unavailable("wait for llama sidecar", error)),
        Err(_) => Err(sidecar_generation_timeout(child).await),
    }
}

fn sidecar_stream_with_cpu_fallback(
    mut sidecar: TokenStream,
    cpu: LlamaCppTextGenerator,
    prompt: String,
) -> TokenStream {
    Box::pin(try_stream! {
        let mut emitted_sidecar_text = false;
        while let Some(piece) = sidecar.next().await {
            match piece {
                Ok(text) => {
                    emitted_sidecar_text = true;
                    yield text;
                }
                Err(error) if !emitted_sidecar_text => {
                    warn!(
                        failure_kind = sidecar_failure_kind(&error),
                        "llama.cpp Vulkan sidecar stream failed before output; falling back to embedded CPU provider"
                    );
                    let mut fallback = cpu.generate(prompt).await?;
                    while let Some(token) = fallback.next().await {
                        yield token?;
                    }
                    return;
                }
                Err(error) => Err(error)?,
            }
        }
    })
}

async fn sidecar_generation_timeout(child: &mut tokio::process::Child) -> PortError {
    let _ = child.start_kill();
    let _ = child.wait().await;
    PortError::Transient("llama sidecar generation timed out".to_owned())
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

fn sidecar_timeout(context: &str) -> PortError {
    PortError::Transient(format!("{context}: timed out"))
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
        GitHubAsset, GitHubRelease, SidecarStdoutSanitizer, asset_matches_windows_vulkan,
        build_llama_cli_args, checked_sidecar_download_size, decode_sidecar_stdout,
        extract_zip_safely, extract_zip_safely_with_limit, find_installed_binary_for_release,
        release_api_url_from_override, sanitize_sidecar_stdout, select_release_asset,
        write_install_metadata,
    };

    fn release(tag: &str, draft: bool, assets: &[&str]) -> GitHubRelease {
        release_with_flags(tag, draft, false, assets)
    }

    fn release_with_flags(
        tag: &str,
        draft: bool,
        prerelease: bool,
        assets: &[&str],
    ) -> GitHubRelease {
        GitHubRelease {
            tag_name: tag.to_owned(),
            draft,
            prerelease,
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
    fn release_selection_skips_prereleases() {
        let releases = vec![
            release_with_flags(
                "b9759",
                false,
                true,
                &["llama-b9759-bin-win-vulkan-x64.zip"],
            ),
            release("b9758", false, &["llama-b9758-bin-win-vulkan-x64.zip"]),
        ];

        let selected = select_release_asset(&releases).expect("asset should be selected");

        assert_eq!(selected.release_tag, "b9758");
        assert_eq!(selected.asset_name, "llama-b9758-bin-win-vulkan-x64.zip");
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
    fn override_release_url_rejects_non_release_forms() {
        for value in [
            "",
            "https://github.com/ggml-org/llama.cpp/releases/tag/",
            "https://github.com/ggml-org/llama.cpp/releases/latest",
            "https://github.com/ggml-org/llama.cpp/releases/download/b9758/llama-b9758-bin-win-vulkan-x64.zip",
            "https://github.com/example/llama.cpp/releases/tag/b9758",
            "https://api.github.com/repos/ggml-org/llama.cpp/releases/tags/",
            "https://api.github.com/repos/ggml-org/llama.cpp/releases/tags/b9758/extra",
        ] {
            assert!(
                release_api_url_from_override(value).is_err(),
                "{value} should not be accepted as a llama.cpp release override"
            );
        }
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

    #[test]
    fn safe_zip_extraction_rejects_archives_over_size_limit() {
        let directory = TempDir::new().unwrap();
        let archive = directory.path().join("llama.zip");
        write_zip(
            &archive,
            &[("llama-b9758-bin-win-vulkan-x64/llama-cli.exe", b"binary")],
        );
        let staging = directory.path().join("staging");

        let error = extract_zip_safely_with_limit(&archive, &staging, 5).unwrap_err();

        assert!(error.to_string().contains("size limit"));
    }

    #[test]
    fn sidecar_stdout_decoder_preserves_split_utf8() {
        let mut decoder = encoding_rs::UTF_8.new_decoder();

        let ascii = decode_sidecar_stdout(&mut decoder, b"hello ", false).unwrap();
        let pending = decode_sidecar_stdout(&mut decoder, &[0xF0, 0x9F], false).unwrap();
        let completed = decode_sidecar_stdout(&mut decoder, &[0x92, 0xA1], false).unwrap();
        let trailing = decode_sidecar_stdout(&mut decoder, &[], true).unwrap();

        assert_eq!(ascii, "hello ");
        assert_eq!(pending, "");
        assert_eq!(completed, "\u{1F4A1}");
        assert_eq!(trailing, "");
    }

    #[test]
    fn sidecar_stdout_sanitizer_removes_llama_cli_transcript_and_prompt() {
        let prompt = "Answer only from local captures.\n\nQuestion: What was visible?";
        let stdout = format!(
            "\nLoading model...\n\nbuild      : b9758\nmodel      : C:\\model.gguf\n\navailable commands:\n  /exit or Ctrl+C     stop or exit\n\n\n> {prompt}\n\nThe visible screen showed a terminal window.\n\nExiting...\n"
        );

        let sanitized = sanitize_sidecar_stdout(&stdout, prompt).unwrap();

        assert_eq!(sanitized, "The visible screen showed a terminal window.");
    }

    #[test]
    fn sidecar_stdout_sanitizer_rejects_ambiguous_interactive_transcript() {
        let prompt = "Sensitive local prompt that must not leak";
        let stdout = "\nLoading model...\n\navailable commands:\n  /exit or Ctrl+C     stop or exit\n\n\n> Sensitive local prompt";

        let error = sanitize_sidecar_stdout(stdout, prompt).unwrap_err();

        assert!(error.to_string().contains("interactive transcript"));
    }

    #[test]
    fn sidecar_streaming_sanitizer_yields_after_prompt_boundary() {
        let prompt = "Answer only from local captures.\n\nQuestion: What was visible?";
        let mut sanitizer = SidecarStdoutSanitizer::new(prompt);
        let mut output = String::new();

        output.push_str(
            &sanitizer
                .push(&format!(
                    "\nLoading model...\n\navailable commands:\n  /exit or Ctrl+C     stop or exit\n\n\n> {prompt}"
                )),
        );
        let first_answer = sanitizer
            .push("\n\nThe visible screen showed a terminal with local logs and status text.");
        assert!(!first_answer.is_empty());
        output.push_str(&first_answer);
        output.push_str(&sanitizer.push("\n\nExiting...\n"));
        output.push_str(&sanitizer.finish().unwrap());

        assert_eq!(
            output,
            "The visible screen showed a terminal with local logs and status text."
        );
    }

    #[test]
    fn sidecar_streaming_sanitizer_passes_clean_output_through() {
        let mut sanitizer = SidecarStdoutSanitizer::new("Question?");
        let mut output = String::new();

        output.push_str(&sanitizer.push("Clean answer text without transcript."));
        output.push_str(&sanitizer.finish().unwrap());

        assert_eq!(output, "Clean answer text without transcript.");
    }

    #[test]
    fn sidecar_streaming_sanitizer_rejects_loading_preamble_without_prompt_boundary() {
        let mut sanitizer = SidecarStdoutSanitizer::new("Sensitive prompt");

        let first = sanitizer.push("\nLoading model...\n\nbuild      : b9758");
        let error = sanitizer.finish().unwrap_err();

        assert_eq!(first, "");
        assert!(error.to_string().contains("interactive transcript"));
    }

    #[test]
    fn sidecar_command_args_keep_prompt_in_file_and_match_cpu_sampling() {
        let args = build_llama_cli_args(
            Path::new("C:/models/local.gguf"),
            Path::new("C:/Temp/prompt.txt"),
            "8",
            "4096",
            "768",
        );

        assert!(
            args.windows(2)
                .any(|pair| pair == ["--file", "C:/Temp/prompt.txt"])
        );
        assert!(!args.iter().any(|arg| arg == "--prompt"));
        assert!(args.windows(2).any(|pair| pair == ["--reasoning", "off"]));
        assert!(args.iter().any(|arg| arg == "--simple-io"));
        assert!(args.windows(2).any(|pair| pair == ["--temp", "0"]));
        assert!(args.windows(2).any(|pair| pair == ["--seed", "0"]));
    }

    #[test]
    fn sidecar_download_size_limit_rejects_oversized_chunks_before_write() {
        let error = checked_sidecar_download_size(8, 5, 12).unwrap_err();

        assert!(error.to_string().contains("download exceeded size limit"));
    }

    #[test]
    fn installed_sidecar_must_match_required_release_tag_when_pinned() {
        let directory = TempDir::new().unwrap();
        let current = directory.path().join("current");
        let nested = current.join("llama-b9758");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("llama-cli.exe"), b"binary").unwrap();
        write_install_metadata(&current, "b9758", "llama-b9758-bin-win-vulkan-x64.zip").unwrap();

        assert!(
            find_installed_binary_for_release(directory.path(), Some("b9758"))
                .unwrap()
                .is_some()
        );
        assert!(
            find_installed_binary_for_release(directory.path(), Some("b9759"))
                .unwrap()
                .is_none()
        );
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
