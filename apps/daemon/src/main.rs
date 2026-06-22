//! Persistent ScreenSearch V2 daemon and named-pipe endpoint.

mod supervisor;

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use async_stream::try_stream;
use futures::{StreamExt, stream};
use screensearch_application::{
    AnalysisService, CapturePolicy, CapturePolicyConfig, IngestService, SearchService,
};
use screensearch_domain::{
    ArchiveSettings, CaptureDisposition, CaptureId, DeleteCaptures, GenerationModel,
    ModelSourceKind, SearchEvent, SearchMatchKind, StorageMetrics,
};
use screensearch_ipc::{
    IpcError, RequestHandler, ResponseStream,
    transport::{
        DEFAULT_PIPE_NAME, DEFAULT_WORKER_PIPE_NAME, IpcClient, WorkerLifeline,
        create_worker_lifeline, serve,
    },
    v1::{
        ArchiveSettingsResponse, CaptureAssetResponse, CaptureResponse, Citation,
        DeleteCapturesResponse, DeleteGenerationModelResponse, ErrorResponse,
        GenerationModel as IpcGenerationModel, GenerationModelResponse, GenerationModelsResponse,
        HealthResponse, NormalizedRect, ProcessJobsResponse, ResponseEnvelope, SearchCompleted,
        SearchEvent as IpcSearchEvent, SetCapturePausedResponse, Token,
        UnloadGenerationModelResponse, UpdateArchiveSettingsResponse, WorkerEmbeddingRequest,
        WorkerGenerationRequest, WorkerHealthRequest, WorkerOcrRequest, WorkerUnloadRequest,
        request_envelope, response_envelope, search_event, worker_generation_event,
    },
};
use screensearch_persistence::{FileAssetStore, LibSqlArchive};
use screensearch_ports::{
    ArchiveRepository, EmbeddingEngine, OcrEngine, PortError, TextGenerator, TokenStream,
};
use screensearch_windows::WindowsGraphicsCaptureSource;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, watch};
use tracing::{error, info, warn};

use crate::supervisor::{RestartDecision, RestartPolicy};

/// Stable OCR revision the worker is expected to report (ADR 0002 model-revision isolation).
const WORKER_OCR_MODEL_ID: &str = "windows-media-ocr-user-profile-v1";
/// Stable embedding revision the worker is expected to report (ADR 0002).
const WORKER_EMBEDDING_MODEL_ID: &str = "fastembed-all-minilm-l6-v2-q-384-v1";
/// Idle interval after which a resident generation model is unloaded (spec §11, ADR 0003).
const GENERATION_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
/// Cadence of the idle-unload check loop.
const GENERATION_IDLE_CHECK_INTERVAL: Duration = Duration::from_secs(30);

struct DaemonHandler {
    ingest: Arc<IngestService>,
    analysis: Arc<AnalysisService>,
    search: Arc<SearchService>,
    repository: Arc<LibSqlArchive>,
    assets: Arc<FileAssetStore>,
    capture_policy: Arc<CapturePolicy>,
    model_root: PathBuf,
}

struct WorkerModelClient {
    repository: Arc<LibSqlArchive>,
    pipe_name: String,
    last_generation: Arc<AtomicU64>,
}

#[async_trait::async_trait]
impl OcrEngine for WorkerModelClient {
    fn model_id(&self) -> &'static str {
        WORKER_OCR_MODEL_ID
    }

    async fn recognize(
        &self,
        asset: &screensearch_domain::AssetRef,
    ) -> Result<Vec<screensearch_domain::OcrBlock>, PortError> {
        let responses = IpcClient::new(&self.pipe_name)
            .request(screensearch_ipc::v1::RequestEnvelope {
                request_id: uuid::Uuid::now_v7().to_string(),
                body: Some(request_envelope::Body::WorkerOcr(WorkerOcrRequest {
                    asset_relative_path: asset.relative_path.clone(),
                    media_type: asset.media_type.clone(),
                })),
            })
            .await
            .map_err(|error| worker_error(&error))?;
        for response in responses {
            match response.body {
                Some(response_envelope::Body::WorkerOcr(result)) => {
                    if result.model_id != WORKER_OCR_MODEL_ID {
                        warn!(
                            reported = %result.model_id,
                            expected = WORKER_OCR_MODEL_ID,
                            "model worker reported an unexpected OCR revision"
                        );
                        return Err(PortError::Internal(format!(
                            "model worker OCR revision {} does not match expected {WORKER_OCR_MODEL_ID}",
                            result.model_id
                        )));
                    }
                    return result
                        .blocks
                        .into_iter()
                        .map(|block| {
                            let bounds = block.bounds.ok_or_else(|| {
                                PortError::InvalidData(
                                    "worker OCR block is missing bounds".to_owned(),
                                )
                            })?;
                            Ok(screensearch_domain::OcrBlock {
                                reading_order: block.reading_order,
                                bounds: screensearch_domain::BoundingBox {
                                    x: bounds.x,
                                    y: bounds.y,
                                    width: bounds.width,
                                    height: bounds.height,
                                }
                                .validate()
                                .map_err(|error| PortError::InvalidData(error.to_string()))?,
                                text: block.text,
                                confidence: block.confidence,
                                language: (!block.language.is_empty()).then_some(block.language),
                            })
                        })
                        .collect();
                }
                Some(response_envelope::Body::Error(error)) => {
                    return Err(PortError::Transient(error.message));
                }
                _ => {}
            }
        }
        Err(PortError::Transient(
            "worker returned no OCR response".to_owned(),
        ))
    }
}

#[async_trait::async_trait]
impl EmbeddingEngine for WorkerModelClient {
    fn model_id(&self) -> &'static str {
        WORKER_EMBEDDING_MODEL_ID
    }

    fn dimensions(&self) -> usize {
        384
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>, PortError> {
        let responses = IpcClient::new(&self.pipe_name)
            .request(screensearch_ipc::v1::RequestEnvelope {
                request_id: uuid::Uuid::now_v7().to_string(),
                body: Some(request_envelope::Body::WorkerEmbedding(
                    WorkerEmbeddingRequest {
                        text: text.to_owned(),
                    },
                )),
            })
            .await
            .map_err(|error| worker_error(&error))?;
        for response in responses {
            match response.body {
                Some(response_envelope::Body::WorkerEmbedding(result)) => {
                    if result.model_id != WORKER_EMBEDDING_MODEL_ID {
                        warn!(
                            reported = %result.model_id,
                            expected = WORKER_EMBEDDING_MODEL_ID,
                            "model worker reported an unexpected embedding revision"
                        );
                        return Err(PortError::Internal(format!(
                            "model worker embedding revision {} does not match expected {WORKER_EMBEDDING_MODEL_ID}",
                            result.model_id
                        )));
                    }
                    return Ok(result.vector);
                }
                Some(response_envelope::Body::Error(error)) => {
                    return Err(PortError::Transient(error.message));
                }
                _ => {}
            }
        }
        Err(PortError::Transient(
            "worker returned no embedding response".to_owned(),
        ))
    }
}

#[async_trait::async_trait]
impl TextGenerator for WorkerModelClient {
    async fn generate(&self, prompt: String) -> Result<TokenStream, PortError> {
        self.last_generation.store(now_millis(), Ordering::Relaxed);
        let model = self.repository.active_generation_model().await?;
        let (model_id, model_relative_path) = model.map_or_else(
            || {
                (
                    "bundled-generator".to_owned(),
                    "generator/model.gguf".to_owned(),
                )
            },
            |model| (model.id, format!("generator/{}", model.relative_path)),
        );
        let pipe_name = self.pipe_name.clone();
        let (send, mut receive) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let request_id = uuid::Uuid::now_v7().to_string();
            let request = screensearch_ipc::v1::RequestEnvelope {
                request_id,
                body: Some(request_envelope::Body::WorkerGeneration(
                    WorkerGenerationRequest {
                        model_id,
                        model_relative_path,
                        prompt,
                    },
                )),
            };
            let result = IpcClient::new(pipe_name)
                .request_each(request, |response| {
                    match response.body {
                        Some(response_envelope::Body::WorkerGeneration(event)) => {
                            if let Some(worker_generation_event::Event::Token(token)) = event.event
                            {
                                send.send(Ok(token.text)).map_err(|_| {
                                    IpcError::Handler(
                                        "generation token consumer disconnected".to_owned(),
                                    )
                                })?;
                            }
                        }
                        Some(response_envelope::Body::Error(error)) => {
                            send.send(Err(PortError::Unavailable(error.message)))
                                .map_err(|_| {
                                    IpcError::Handler(
                                        "generation error consumer disconnected".to_owned(),
                                    )
                                })?;
                        }
                        _ => {}
                    }
                    Ok(())
                })
                .await;
            if let Err(error) = result {
                let _ = send.send(Err(PortError::Unavailable(format!(
                    "model worker: {error}"
                ))));
            }
        });
        Ok(Box::pin(try_stream! {
            while let Some(token) = receive.recv().await {
                yield token?;
            }
        }))
    }
}

fn worker_error(error: &IpcError) -> PortError {
    PortError::Transient(format!("model worker: {error}"))
}

async fn unload_model_worker() -> Result<(), anyhow::Error> {
    let responses = IpcClient::new(DEFAULT_WORKER_PIPE_NAME)
        .request(screensearch_ipc::v1::RequestEnvelope {
            request_id: uuid::Uuid::now_v7().to_string(),
            body: Some(request_envelope::Body::WorkerUnload(WorkerUnloadRequest {})),
        })
        .await
        .context("request model worker unload")?;
    for response in responses {
        match response.body {
            Some(response_envelope::Body::WorkerUnload(_)) => return Ok(()),
            Some(response_envelope::Body::Error(error)) => anyhow::bail!(error.message),
            _ => {}
        }
    }
    anyhow::bail!("model worker returned no unload response")
}

async fn wait_for_model_worker_ready(timeout: Duration) -> Result<(), anyhow::Error> {
    let started = std::time::Instant::now();
    let mut last_error = None;
    while started.elapsed() < timeout {
        match IpcClient::new(DEFAULT_WORKER_PIPE_NAME)
            .request(screensearch_ipc::v1::RequestEnvelope {
                request_id: uuid::Uuid::now_v7().to_string(),
                body: Some(request_envelope::Body::WorkerHealth(WorkerHealthRequest {})),
            })
            .await
        {
            Ok(responses) => {
                for response in responses {
                    match response.body {
                        Some(response_envelope::Body::WorkerHealth(_)) => return Ok(()),
                        Some(response_envelope::Body::Error(error)) => {
                            last_error = Some(error.message);
                        }
                        _ => {}
                    }
                }
            }
            Err(error) => last_error = Some(error.to_string()),
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    anyhow::bail!(
        "model worker did not become ready within {}s: {}",
        timeout.as_secs(),
        last_error.unwrap_or_else(|| "no response".to_owned())
    )
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

async fn worker_generation_loaded() -> Result<bool, anyhow::Error> {
    let responses = IpcClient::new(DEFAULT_WORKER_PIPE_NAME)
        .request(screensearch_ipc::v1::RequestEnvelope {
            request_id: uuid::Uuid::now_v7().to_string(),
            body: Some(request_envelope::Body::WorkerHealth(WorkerHealthRequest {})),
        })
        .await
        .context("probe model worker health")?;
    for response in responses {
        match response.body {
            Some(response_envelope::Body::WorkerHealth(health)) => {
                return Ok(health.generation_loaded);
            }
            Some(response_envelope::Body::Error(error)) => anyhow::bail!(error.message),
            _ => {}
        }
    }
    anyhow::bail!("model worker returned no health response")
}

/// Unloads the resident generation model after it has been idle past the timeout.
///
/// Sends a raw worker unload (not the daemon `UnloadGenerationModel` handler, which also
/// clears the catalog selection): the active model stays selected so the next query
/// reloads it lazily, satisfying the spec §11 / ADR 0003 memory lifecycle.
async fn idle_unload_loop(last_generation: Arc<AtomicU64>, mut shutdown: watch::Receiver<bool>) {
    let mut cadence = tokio::time::interval(GENERATION_IDLE_CHECK_INTERVAL);
    cadence.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let idle_timeout_millis =
        u64::try_from(GENERATION_IDLE_TIMEOUT.as_millis()).unwrap_or(u64::MAX);
    loop {
        tokio::select! {
            _ = cadence.tick() => {
                let last = last_generation.load(Ordering::Relaxed);
                if last == 0 || now_millis().saturating_sub(last) < idle_timeout_millis {
                    continue;
                }
                match worker_generation_loaded().await {
                    Ok(true) => match unload_model_worker().await {
                        Ok(()) => info!("unloaded idle generation model"),
                        Err(error) => warn!(error = %error, "idle generation-model unload failed"),
                    },
                    Ok(false) => {}
                    Err(error) => warn!(error = %error, "idle-unload health probe failed"),
                }
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return;
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl RequestHandler for DaemonHandler {
    #[allow(clippy::too_many_lines)]
    async fn handle(
        &self,
        request: screensearch_ipc::v1::RequestEnvelope,
    ) -> Result<ResponseStream, IpcError> {
        let request_id = request.request_id;
        let Some(body) = request.body else {
            return Ok(single_response(error_response(
                request_id,
                "invalid_request",
                "request body is missing",
                false,
            )));
        };

        match body {
            request_envelope::Body::Health(_) => {
                let metrics = match self.repository.queue_metrics().await {
                    Ok(metrics) => metrics,
                    Err(error) => {
                        return Ok(single_response(error_response(
                            request_id,
                            "health_failed",
                            &error.to_string(),
                            true,
                        )));
                    }
                };
                let storage = match self.repository.storage_metrics().await {
                    Ok(metrics) => metrics,
                    Err(error) => {
                        return Ok(single_response(error_response(
                            request_id,
                            "health_failed",
                            &error.to_string(),
                            true,
                        )));
                    }
                };
                let capture_state = if self.capture_policy.is_paused() {
                    "paused"
                } else if self.capture_policy.is_backpressured() {
                    "backpressured"
                } else {
                    "capturing"
                };
                Ok(single_response(ResponseEnvelope {
                    request_id,
                    terminal: true,
                    body: Some(response_envelope::Body::Health(HealthResponse {
                        version: env!("CARGO_PKG_VERSION").to_owned(),
                        status: "ready".to_owned(),
                        capture_paused: self.capture_policy.is_paused(),
                        capture_state: capture_state.to_owned(),
                        queue_depth: metrics.depth(),
                        oldest_pending_age_seconds: metrics.oldest_pending_age_seconds,
                        retry_count: metrics.retry_count,
                        dead_letter_count: metrics.dead_letter_count,
                        queue_high_water: self.capture_policy.queue_high_water(),
                        capture_count: storage.capture_count,
                        asset_bytes: storage.asset_bytes,
                        ocr_block_count: storage.ocr_block_count,
                        search_chunk_count: storage.search_chunk_count,
                    })),
                }))
            }
            request_envelope::Body::Capture(_) => {
                let response = match self.ingest.capture_once().await {
                    Ok(CaptureDisposition::Enqueued { capture_id, .. }) => ResponseEnvelope {
                        request_id,
                        terminal: true,
                        body: Some(response_envelope::Body::Capture(CaptureResponse {
                            capture_id: capture_id.to_string(),
                            duplicate: false,
                            skipped_reason: String::new(),
                        })),
                    },
                    Ok(CaptureDisposition::Duplicate { capture_id }) => ResponseEnvelope {
                        request_id,
                        terminal: true,
                        body: Some(response_envelope::Body::Capture(CaptureResponse {
                            capture_id: capture_id.to_string(),
                            duplicate: true,
                            skipped_reason: String::new(),
                        })),
                    },
                    Ok(CaptureDisposition::Skipped { reason }) => ResponseEnvelope {
                        request_id,
                        terminal: true,
                        body: Some(response_envelope::Body::Capture(CaptureResponse {
                            capture_id: String::new(),
                            duplicate: false,
                            skipped_reason: reason.to_string(),
                        })),
                    },
                    Err(error) => {
                        error_response(request_id, "capture_failed", &error.to_string(), true)
                    }
                };
                Ok(single_response(response))
            }
            request_envelope::Body::ProcessJobs(command) => {
                let maximum = command.maximum.clamp(1, 100);
                let mut processed = 0;
                let mut failure = None;
                for _ in 0..maximum {
                    match self.analysis.process_one().await {
                        Ok(true) => processed += 1,
                        Ok(false) => break,
                        Err(error) => {
                            failure = Some(error);
                            break;
                        }
                    }
                }
                let response = failure.map_or_else(
                    || ResponseEnvelope {
                        request_id: request_id.clone(),
                        terminal: true,
                        body: Some(response_envelope::Body::ProcessJobs(ProcessJobsResponse {
                            processed,
                        })),
                    },
                    |error| {
                        error_response(
                            request_id.clone(),
                            "analysis_failed",
                            &error.to_string(),
                            true,
                        )
                    },
                );
                Ok(single_response(response))
            }
            request_envelope::Body::Search(command) => {
                let events = match self
                    .search
                    .search_with_options(
                        &command.query,
                        command.limit as usize,
                        command.generate_answer,
                    )
                    .await
                {
                    Ok(events) => events,
                    Err(error) => {
                        return Ok(single_response(error_response(
                            request_id,
                            "search_failed",
                            &error.to_string(),
                            false,
                        )));
                    }
                };
                let responses = events.map(move |event| {
                    let event = event.map_err(|error| IpcError::Handler(error.to_string()))?;
                    let (event, terminal) = map_search_event(event);
                    Ok(ResponseEnvelope {
                        request_id: request_id.clone(),
                        terminal,
                        body: Some(response_envelope::Body::Search(IpcSearchEvent {
                            event: Some(event),
                        })),
                    })
                });
                Ok(Box::pin(responses))
            }
            request_envelope::Body::GetCaptureAsset(command) => {
                let capture_id = match uuid::Uuid::parse_str(&command.capture_id) {
                    Ok(value) => CaptureId(value),
                    Err(error) => {
                        return Ok(single_response(error_response(
                            request_id,
                            "invalid_capture_id",
                            &error.to_string(),
                            false,
                        )));
                    }
                };
                let Some(asset) = self
                    .repository
                    .capture_asset(capture_id)
                    .await
                    .map_err(|error| IpcError::Handler(error.to_string()))?
                else {
                    return Ok(single_response(error_response(
                        request_id,
                        "capture_not_found",
                        "capture does not exist",
                        false,
                    )));
                };
                if asset.byte_length > 16 * 1024 * 1024 {
                    return Ok(single_response(error_response(
                        request_id,
                        "asset_too_large",
                        "capture asset exceeds the IPC preview limit",
                        false,
                    )));
                }
                let path = self
                    .assets
                    .resolve(&asset)
                    .map_err(|error| IpcError::Handler(error.to_string()))?;
                let content = tokio::fs::read(path)
                    .await
                    .map_err(|error| IpcError::Handler(format!("read capture asset: {error}")))?;
                Ok(single_response(ResponseEnvelope {
                    request_id,
                    terminal: true,
                    body: Some(response_envelope::Body::CaptureAsset(
                        CaptureAssetResponse {
                            capture_id: capture_id.to_string(),
                            media_type: asset.media_type,
                            content,
                        },
                    )),
                }))
            }
            request_envelope::Body::SetCapturePaused(command) => {
                self.capture_policy.set_paused(command.paused);
                info!(paused = command.paused, "automatic capture state changed");
                Ok(single_response(ResponseEnvelope {
                    request_id,
                    terminal: true,
                    body: Some(response_envelope::Body::SetCapturePaused(
                        SetCapturePausedResponse {
                            paused: command.paused,
                        },
                    )),
                }))
            }
            request_envelope::Body::GetArchiveSettings(_) => {
                let settings = self
                    .ingest
                    .archive_settings()
                    .await
                    .map_err(|error| IpcError::Handler(error.to_string()))?;
                let metrics = self
                    .ingest
                    .storage_metrics()
                    .await
                    .map_err(|error| IpcError::Handler(error.to_string()))?;
                Ok(single_response(ResponseEnvelope {
                    request_id,
                    terminal: true,
                    body: Some(response_envelope::Body::ArchiveSettings(
                        map_archive_settings(settings, metrics),
                    )),
                }))
            }
            request_envelope::Body::UpdateArchiveSettings(command) => {
                let settings = ArchiveSettings {
                    retention_days: command.retention_days,
                    disk_budget_bytes: command.disk_budget_bytes,
                    excluded_applications: command.excluded_applications,
                    excluded_titles: command.excluded_titles,
                };
                if let Err(error) = self.ingest.update_archive_settings(settings).await {
                    return Ok(single_response(error_response(
                        request_id,
                        "invalid_archive_settings",
                        &error.to_string(),
                        false,
                    )));
                }
                let deleted = self
                    .ingest
                    .run_retention(chrono::Utc::now())
                    .await
                    .map_err(|error| IpcError::Handler(error.to_string()))?;
                let settings = self
                    .ingest
                    .archive_settings()
                    .await
                    .map_err(|error| IpcError::Handler(error.to_string()))?;
                let metrics = self
                    .ingest
                    .storage_metrics()
                    .await
                    .map_err(|error| IpcError::Handler(error.to_string()))?;
                Ok(single_response(ResponseEnvelope {
                    request_id,
                    terminal: true,
                    body: Some(response_envelope::Body::UpdateArchiveSettings(
                        UpdateArchiveSettingsResponse {
                            settings: Some(map_archive_settings(settings, metrics)),
                            captures_deleted: deleted.captures_deleted,
                            assets_scheduled: deleted.assets_scheduled,
                        },
                    )),
                }))
            }
            request_envelope::Body::DeleteCaptures(command) => {
                if command.delete_all && !command.confirmed {
                    return Ok(single_response(error_response(
                        request_id,
                        "confirmation_required",
                        "deleting all captured history requires explicit confirmation",
                        false,
                    )));
                }
                let mut capture_ids = Vec::with_capacity(command.capture_ids.len());
                for value in command.capture_ids {
                    match uuid::Uuid::parse_str(&value) {
                        Ok(value) => capture_ids.push(CaptureId(value)),
                        Err(error) => {
                            return Ok(single_response(error_response(
                                request_id,
                                "invalid_capture_id",
                                &error.to_string(),
                                false,
                            )));
                        }
                    }
                }
                let before = if command.before.trim().is_empty() {
                    None
                } else {
                    match chrono::DateTime::parse_from_rfc3339(&command.before) {
                        Ok(value) => Some(value.with_timezone(&chrono::Utc)),
                        Err(error) => {
                            return Ok(single_response(error_response(
                                request_id,
                                "invalid_delete_range",
                                &error.to_string(),
                                false,
                            )));
                        }
                    }
                };
                if command.delete_all {
                    self.capture_policy.set_paused(true);
                }
                let deleted = self
                    .ingest
                    .delete_captures(DeleteCaptures {
                        capture_ids,
                        before,
                        delete_all: command.delete_all,
                    })
                    .await
                    .map_err(|error| IpcError::Handler(error.to_string()))?;
                Ok(single_response(ResponseEnvelope {
                    request_id,
                    terminal: true,
                    body: Some(response_envelope::Body::DeleteCaptures(
                        DeleteCapturesResponse {
                            captures_deleted: deleted.captures_deleted,
                            assets_scheduled: deleted.assets_scheduled,
                        },
                    )),
                }))
            }
            request_envelope::Body::ListGenerationModels(_) => {
                let models = self
                    .repository
                    .generation_models()
                    .await
                    .map_err(|error| IpcError::Handler(error.to_string()))?;
                Ok(single_response(ResponseEnvelope {
                    request_id,
                    terminal: true,
                    body: Some(response_envelope::Body::GenerationModels(
                        GenerationModelsResponse {
                            models: models.into_iter().map(map_generation_model).collect(),
                        },
                    )),
                }))
            }
            request_envelope::Body::ImportLocalGenerationModel(command) => {
                let response = match import_generation_model(
                    &self.model_root,
                    &command.source_path,
                    &command.display_name,
                    command.select,
                    &self.repository,
                )
                .await
                {
                    Ok(model) => ResponseEnvelope {
                        request_id,
                        terminal: true,
                        body: Some(response_envelope::Body::GenerationModel(
                            GenerationModelResponse {
                                model: Some(map_generation_model(model)),
                            },
                        )),
                    },
                    Err(error) => {
                        error_response(request_id, "model_import_failed", &error.to_string(), false)
                    }
                };
                Ok(single_response(response))
            }
            request_envelope::Body::DownloadGenerationModel(command) => {
                let response = match download_generation_model(
                    &self.model_root,
                    &command.repository,
                    &command.filename,
                    &command.display_name,
                    command.select,
                    &self.repository,
                )
                .await
                {
                    Ok(model) => ResponseEnvelope {
                        request_id,
                        terminal: true,
                        body: Some(response_envelope::Body::GenerationModel(
                            GenerationModelResponse {
                                model: Some(map_generation_model(model)),
                            },
                        )),
                    },
                    Err(error) => error_response(
                        request_id,
                        "model_download_failed",
                        &error.to_string(),
                        true,
                    ),
                };
                Ok(single_response(response))
            }
            request_envelope::Body::SelectGenerationModel(command) => {
                if let Err(error) = self
                    .repository
                    .select_generation_model(&command.model_id)
                    .await
                {
                    return Ok(single_response(error_response(
                        request_id,
                        "model_select_failed",
                        &error.to_string(),
                        false,
                    )));
                }
                let model = self
                    .repository
                    .active_generation_model()
                    .await
                    .map_err(|error| IpcError::Handler(error.to_string()))?
                    .ok_or_else(|| IpcError::Handler("active model is missing".to_owned()))?;
                Ok(single_response(ResponseEnvelope {
                    request_id,
                    terminal: true,
                    body: Some(response_envelope::Body::GenerationModel(
                        GenerationModelResponse {
                            model: Some(map_generation_model(model)),
                        },
                    )),
                }))
            }
            request_envelope::Body::DeleteGenerationModel(command) => {
                let deleted =
                    delete_generation_model(&self.model_root, &command.model_id, &self.repository)
                        .await
                        .map_err(|error| IpcError::Handler(error.to_string()))?;
                Ok(single_response(ResponseEnvelope {
                    request_id,
                    terminal: true,
                    body: Some(response_envelope::Body::DeleteGenerationModel(
                        DeleteGenerationModelResponse { deleted },
                    )),
                }))
            }
            request_envelope::Body::UnloadGenerationModel(_)
            | request_envelope::Body::WorkerUnload(_) => {
                if let Err(error) = self.repository.clear_active_generation_model().await {
                    return Ok(single_response(error_response(
                        request_id,
                        "model_unload_failed",
                        &error.to_string(),
                        false,
                    )));
                }
                if let Err(error) = unload_model_worker().await {
                    return Ok(single_response(error_response(
                        request_id,
                        "model_unload_failed",
                        &error.to_string(),
                        true,
                    )));
                }
                Ok(single_response(ResponseEnvelope {
                    request_id,
                    terminal: true,
                    body: Some(response_envelope::Body::UnloadGenerationModel(
                        UnloadGenerationModelResponse { unloaded: true },
                    )),
                }))
            }
            request_envelope::Body::WorkerHealth(_)
            | request_envelope::Body::WorkerOcr(_)
            | request_envelope::Body::WorkerEmbedding(_)
            | request_envelope::Body::WorkerGeneration(_) => Ok(single_response(error_response(
                request_id,
                "wrong_endpoint",
                "worker request was sent to the daemon endpoint",
                false,
            ))),
        }
    }
}

fn map_archive_settings(
    settings: ArchiveSettings,
    metrics: StorageMetrics,
) -> ArchiveSettingsResponse {
    ArchiveSettingsResponse {
        retention_days: settings.retention_days,
        disk_budget_bytes: settings.disk_budget_bytes,
        excluded_applications: settings.excluded_applications,
        excluded_titles: settings.excluded_titles,
        capture_count: metrics.capture_count,
        asset_bytes: metrics.asset_bytes,
    }
}

fn map_search_event(event: SearchEvent) -> (search_event::Event, bool) {
    match event {
        SearchEvent::Citation(hit) => (
            search_event::Event::Citation(Box::new(Citation {
                capture_id: hit.capture_id.to_string(),
                chunk_id: hit.chunk_id.to_string(),
                excerpt: hit.text,
                score: hit.score,
                captured_at: hit.captured_at.to_rfc3339(),
                application: hit.application,
                window_title: hit.window_title,
                width: hit.width,
                height: hit.height,
                bounds: hit
                    .bounds
                    .into_iter()
                    .map(|bounds| NormalizedRect {
                        x: bounds.x,
                        y: bounds.y,
                        width: bounds.width,
                        height: bounds.height,
                    })
                    .collect(),
                match_kind: match hit.match_kind {
                    SearchMatchKind::Lexical => "lexical",
                    SearchMatchKind::Semantic => "semantic",
                    SearchMatchKind::Hybrid => "hybrid",
                }
                .to_owned(),
                ocr_model_id: hit.ocr_model_id,
                embedding_model_id: hit.embedding_model_id,
            })),
            false,
        ),
        SearchEvent::Token(text) => (search_event::Event::Token(Token { text }), false),
        SearchEvent::Completed {
            citation_count,
            answer_status,
            answer_message,
        } => (
            search_event::Event::Completed(SearchCompleted {
                citation_count: u32::try_from(citation_count).unwrap_or(u32::MAX),
                answer_status,
                answer_message: answer_message.unwrap_or_default(),
            }),
            true,
        ),
    }
}

fn map_generation_model(model: GenerationModel) -> IpcGenerationModel {
    IpcGenerationModel {
        id: model.id,
        display_name: model.display_name,
        source: model.source.as_str().to_owned(),
        repository: model.repository.unwrap_or_default(),
        filename: model.filename,
        relative_path: model.relative_path,
        content_hash: model.content_hash.unwrap_or_default(),
        byte_length: model.byte_length,
        architecture: model.architecture.unwrap_or_default(),
        quantization: model.quantization.unwrap_or_default(),
        context_tokens: model.context_tokens.unwrap_or_default(),
        supports_vision: model.supports_vision,
        active: model.active,
    }
}

async fn import_generation_model(
    model_root: &Path,
    source_path: &str,
    display_name: &str,
    select: bool,
    repository: &Arc<LibSqlArchive>,
) -> Result<GenerationModel, anyhow::Error> {
    let source = resolve_model_source_path(source_path)?;
    let filename = source
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow::anyhow!("source model file has no filename"))?
        .to_owned();
    let model_id = model_id(display_name, &filename);
    let relative_path = format!("{model_id}/{filename}");
    let target = model_root.join(&relative_path);
    let (byte_length, content_hash) = copy_and_hash(&source, &target).await?;
    let model = generation_model_from_file(
        model_id,
        display_name,
        ModelSourceKind::Local,
        None,
        &filename,
        relative_path,
        content_hash,
        byte_length,
        select,
    );
    repository
        .upsert_generation_model(model.clone())
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    Ok(model)
}

fn resolve_model_source_path(source_path: &str) -> Result<PathBuf, anyhow::Error> {
    let trimmed = source_path.trim();
    if trimmed.is_empty() {
        anyhow::bail!("source model path is required");
    }
    let raw = PathBuf::from(trimmed);
    if raw.is_absolute() || raw.exists() {
        return Ok(raw);
    }

    let mut roots = Vec::new();
    if let Ok(current_dir) = std::env::current_dir() {
        roots.push(current_dir);
    }
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    if let Ok(executable) = std::env::current_exe()
        && let Some(parent) = executable.parent()
    {
        roots.push(parent.to_path_buf());
    }

    for root in roots {
        for ancestor in root.ancestors() {
            let candidate = ancestor.join(&raw);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    Ok(raw)
}

async fn download_generation_model(
    model_root: &Path,
    hf_repository: &str,
    filename: &str,
    display_name: &str,
    select: bool,
    repository: &Arc<LibSqlArchive>,
) -> Result<GenerationModel, anyhow::Error> {
    let hf_repository = hf_repository.trim();
    let filename = filename.trim();
    if hf_repository.is_empty() || filename.is_empty() {
        anyhow::bail!("Hugging Face repository and filename are required");
    }
    validate_plain_filename(filename)?;
    let model_id = model_id(display_name, filename);
    let relative_path = format!("{model_id}/{filename}");
    let target = model_root.join(&relative_path);
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let temporary = target.with_extension("download");
    let url = format!("https://huggingface.co/{hf_repository}/resolve/main/{filename}");
    let (byte_length, content_hash) = match download_and_hash(&url, &temporary).await {
        Ok(result) => result,
        Err(error) => {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(error);
        }
    };
    let _ = tokio::fs::remove_file(&target).await;
    if let Err(error) = tokio::fs::rename(&temporary, &target).await {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(error.into());
    }
    let model = generation_model_from_file(
        model_id,
        display_name,
        ModelSourceKind::HuggingFace,
        Some(hf_repository.to_owned()),
        filename,
        relative_path,
        content_hash,
        byte_length,
        select,
    );
    repository
        .upsert_generation_model(model.clone())
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    Ok(model)
}

async fn delete_generation_model(
    model_root: &Path,
    model_id: &str,
    repository: &Arc<LibSqlArchive>,
) -> Result<bool, anyhow::Error> {
    let Some(model) = repository
        .delete_generation_model(model_id)
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?
    else {
        return Ok(false);
    };
    let path = model_root.join(&model.relative_path);
    match tokio::fs::remove_file(&path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    // Best-effort cleanup of the now-empty per-model directory (`{model_root}/{model_id}/`);
    // a still-populated or busy directory is left in place rather than failing the delete.
    if let Some(parent) = path.parent() {
        match tokio::fs::remove_dir(parent).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => warn!(error = %error, "left model directory in place after delete"),
        }
    }
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn generation_model_from_file(
    id: String,
    display_name: &str,
    source: ModelSourceKind,
    repository: Option<String>,
    filename: &str,
    relative_path: String,
    content_hash: String,
    byte_length: u64,
    active: bool,
) -> GenerationModel {
    let quantization = infer_quantization(filename);
    GenerationModel {
        id,
        display_name: if display_name.trim().is_empty() {
            filename.trim_end_matches(".gguf").to_owned()
        } else {
            display_name.trim().to_owned()
        },
        source,
        repository,
        filename: filename.to_owned(),
        relative_path,
        content_hash: Some(content_hash),
        byte_length,
        architecture: infer_architecture(filename),
        quantization,
        context_tokens: Some(2_048),
        supports_vision: filename.to_lowercase().contains("vl") || filename.contains("mmproj"),
        active,
    }
}

fn validate_plain_filename(filename: &str) -> Result<(), anyhow::Error> {
    if Path::new(filename)
        .file_name()
        .and_then(|value| value.to_str())
        != Some(filename)
    {
        anyhow::bail!("filename must be a plain file name without path separators");
    }
    Ok(())
}

async fn copy_and_hash(source: &Path, target: &Path) -> Result<(u64, String), anyhow::Error> {
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let temporary = target.with_extension("import");
    let result = copy_to_temporary_and_hash(source, &temporary).await;
    let (byte_length, content_hash) = match result {
        Ok(result) => result,
        Err(error) => {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(error);
        }
    };
    let _ = tokio::fs::remove_file(target).await;
    if let Err(error) = tokio::fs::rename(&temporary, target).await {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(error.into());
    }
    Ok((byte_length, content_hash))
}

async fn copy_to_temporary_and_hash(
    source: &Path,
    temporary: &Path,
) -> Result<(u64, String), anyhow::Error> {
    let mut input = tokio::fs::File::open(source).await?;
    let mut output = tokio::fs::File::create(temporary).await?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut byte_length = 0_u64;
    loop {
        let read = input.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        output.write_all(chunk).await?;
        hasher.update(chunk);
        byte_length = byte_length.saturating_add(read as u64);
    }
    output.flush().await?;
    drop(output);
    Ok((byte_length, hasher.finalize().to_hex().to_string()))
}

async fn download_and_hash(url: &str, temporary: &Path) -> Result<(u64, String), anyhow::Error> {
    let mut response = reqwest::get(url).await?.error_for_status()?;
    // Reject HTML/text error pages (for example a gated-model authentication wall) before
    // writing a bogus file that would only fail later when llama.cpp rejects the GGUF magic.
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if content_type.starts_with("text/") {
        anyhow::bail!(
            "model download returned a \"{content_type}\" page instead of a model file; the model may require Hugging Face authentication or may not exist"
        );
    }
    let mut output = tokio::fs::File::create(temporary).await?;
    let mut hasher = blake3::Hasher::new();
    let mut byte_length = 0_u64;
    while let Some(chunk) = response.chunk().await? {
        hasher.update(&chunk);
        output.write_all(&chunk).await?;
        byte_length = byte_length.saturating_add(chunk.len() as u64);
    }
    output.flush().await?;
    drop(output);
    Ok((byte_length, hasher.finalize().to_hex().to_string()))
}

fn model_id(display_name: &str, filename: &str) -> String {
    let base = if display_name.trim().is_empty() {
        filename.trim_end_matches(".gguf")
    } else {
        display_name.trim()
    };
    let mut id = base
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while id.contains("--") {
        id = id.replace("--", "-");
    }
    let slug = id.trim_matches('-').chars().take(72).collect::<String>();
    let slug = if slug.is_empty() { "model" } else { &slug };
    format!("{slug}-{}", uuid::Uuid::now_v7().simple())
}

fn infer_architecture(filename: &str) -> Option<String> {
    let lowercase = filename.to_lowercase();
    [
        ("nemotron", "Nemotron"),
        ("ministral", "Ministral"),
        ("qwen", "Qwen"),
        ("gemma", "Gemma"),
        ("phi", "Phi"),
        ("smollm", "SmolLM"),
    ]
    .iter()
    .find_map(|(needle, label)| lowercase.contains(needle).then(|| (*label).to_owned()))
}

fn infer_quantization(filename: &str) -> Option<String> {
    let uppercase = filename.to_ascii_uppercase();
    ["Q4_K_M", "Q4_K_S", "Q5_K_M", "Q6_K", "Q8_0", "BF16", "F16"]
        .iter()
        .find_map(|value| uppercase.contains(value).then(|| (*value).to_owned()))
}

fn single_response(response: ResponseEnvelope) -> ResponseStream {
    Box::pin(stream::once(async move { Ok(response) }))
}

fn error_response(
    request_id: String,
    code: &str,
    message: &str,
    retryable: bool,
) -> ResponseEnvelope {
    ResponseEnvelope {
        request_id,
        terminal: true,
        body: Some(response_envelope::Body::Error(ErrorResponse {
            code: code.to_owned(),
            message: message.to_owned(),
            retryable,
        })),
    }
}

/// Builds the capture policy, services, and request handler from the wired adapters.
async fn compose_handler(
    repository: Arc<LibSqlArchive>,
    assets: Arc<FileAssetStore>,
    worker_client: Arc<WorkerModelClient>,
    persisted_settings: ArchiveSettings,
    generator_root: PathBuf,
) -> anyhow::Result<Arc<DaemonHandler>> {
    let capture_policy = Arc::new(CapturePolicy::new(CapturePolicyConfig {
        queue_high_water: 100,
        queue_low_water: 50,
        excluded_applications: vec!["screensearch".to_owned()],
        excluded_titles: Vec::new(),
    })?);
    capture_policy
        .replace_exclusions(
            persisted_settings.excluded_applications,
            persisted_settings.excluded_titles,
        )
        .await;
    let ingest = Arc::new(IngestService::with_policy(
        Arc::new(WindowsGraphicsCaptureSource),
        assets.clone(),
        repository.clone(),
        capture_policy.clone(),
    ));
    let analysis = Arc::new(AnalysisService::new(
        repository.clone(),
        worker_client.clone(),
        worker_client.clone(),
        "daemon-windows-worker",
    ));
    Ok(Arc::new(DaemonHandler {
        ingest,
        analysis,
        search: Arc::new(SearchService::new(
            repository.clone(),
            worker_client.clone(),
            worker_client,
        )),
        repository,
        assets,
        capture_policy,
        model_root: generator_root,
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "screensearch_daemon=info".into()),
        )
        .try_init()
        .map_err(|error| anyhow::anyhow!("initialize daemon tracing: {error}"))?;

    let data_directory = data_directory()?;
    tokio::fs::create_dir_all(&data_directory)
        .await
        .context("create ScreenSearch data directory")?;
    let repository = Arc::new(
        LibSqlArchive::open(data_directory.join("screensearch.db"))
            .await
            .context("open archive database")?,
    );
    repository
        .migrate()
        .await
        .context("migrate archive database")?;
    let persisted_settings = repository
        .archive_settings()
        .await
        .context("load archive settings")?;

    let asset_root = data_directory.join("assets");
    let assets = Arc::new(FileAssetStore::new(&asset_root));
    let model_root = data_directory.join("models");
    let generator_root = model_root.join("generator");

    let worker_args = WorkerSpawnArgs {
        binary: resolve_worker_binary().context("locate model worker binary")?,
        asset_root: asset_root.clone(),
        model_root: model_root.clone(),
        pipe_name: DEFAULT_WORKER_PIPE_NAME.to_owned(),
        lifeline_pipe_name: format!(
            r"\\.\pipe\screensearch-v2-lifeline-{}",
            uuid::Uuid::now_v7().simple()
        ),
        data_directory: data_directory.to_str().map(str::to_owned),
    };
    let (worker_child, worker_lifeline) = start_worker(&worker_args, Duration::from_secs(30))
        .await
        .context("launch model worker")?;

    let last_generation = Arc::new(AtomicU64::new(0));
    let worker_client = Arc::new(WorkerModelClient {
        repository: repository.clone(),
        pipe_name: DEFAULT_WORKER_PIPE_NAME.to_owned(),
        last_generation: last_generation.clone(),
    });
    let handler = compose_handler(
        repository,
        assets,
        worker_client,
        persisted_settings,
        generator_root,
    )
    .await?;

    info!(pipe = DEFAULT_PIPE_NAME, "daemon ready");
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let capture_task = tokio::spawn(capture_loop(handler.ingest.clone(), shutdown_rx.clone()));
    let analysis_task = tokio::spawn(analysis_loop(handler.analysis.clone(), shutdown_rx.clone()));
    let maintenance_task = tokio::spawn(maintenance_loop(
        handler.ingest.clone(),
        shutdown_rx.clone(),
    ));
    let idle_task = tokio::spawn(idle_unload_loop(last_generation, shutdown_rx.clone()));
    let supervisor_task = tokio::spawn(worker_supervisor_loop(
        worker_args,
        worker_child,
        worker_lifeline,
        shutdown_tx.clone(),
        shutdown_rx.clone(),
    ));
    tokio::select! {
        result = serve(DEFAULT_PIPE_NAME, handler) => result.context("serve named pipe")?,
        result = tokio::signal::ctrl_c() => result.context("wait for shutdown signal")?,
        () = wait_for_shutdown(shutdown_rx.clone()) => {
            warn!("worker supervision requested daemon shutdown");
        }
    }
    let _ = shutdown_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(3), capture_task).await;
    let _ = tokio::time::timeout(Duration::from_secs(3), analysis_task).await;
    let _ = tokio::time::timeout(Duration::from_secs(3), maintenance_task).await;
    let _ = tokio::time::timeout(Duration::from_secs(3), idle_task).await;
    let supervisor_result = tokio::time::timeout(Duration::from_secs(8), supervisor_task).await;
    info!("daemon stopped");
    if matches!(supervisor_result, Ok(Ok(true))) {
        anyhow::bail!("model worker exceeded its restart budget");
    }
    Ok(())
}

async fn capture_loop(ingest: Arc<IngestService>, mut shutdown: watch::Receiver<bool>) {
    let mut cadence = tokio::time::interval(Duration::from_secs(2));
    cadence.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = cadence.tick() => {
                if let Err(error) = ingest.capture_once().await {
                    warn!(error = %error, "automatic capture failed");
                }
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return;
                }
            }
        }
    }
}

async fn analysis_loop(analysis: Arc<AnalysisService>, mut shutdown: watch::Receiver<bool>) {
    loop {
        tokio::select! {
            result = analysis.process_one() => {
                match result {
                    Ok(true) => {}
                    Ok(false) => tokio::time::sleep(Duration::from_millis(200)).await,
                    Err(error) => {
                        warn!(error = %error, "automatic analysis failed");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return;
                }
            }
        }
    }
}

async fn maintenance_loop(ingest: Arc<IngestService>, mut shutdown: watch::Receiver<bool>) {
    let mut cadence = tokio::time::interval(Duration::from_secs(60));
    cadence.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = cadence.tick() => {
                match ingest.run_retention(chrono::Utc::now()).await {
                    Ok(summary) if summary.captures_deleted > 0 => {
                        info!(
                            captures_deleted = summary.captures_deleted,
                            assets_scheduled = summary.assets_scheduled,
                            "archive retention completed"
                        );
                    }
                    Ok(_) => {}
                    Err(error) => warn!(error = %error, "archive retention failed"),
                }
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return;
                }
            }
        }
    }
}

/// Everything needed to (re)spawn the model worker, owned so the supervisor can restart it.
struct WorkerSpawnArgs {
    binary: PathBuf,
    asset_root: PathBuf,
    model_root: PathBuf,
    pipe_name: String,
    lifeline_pipe_name: String,
    data_directory: Option<String>,
}

impl WorkerSpawnArgs {
    fn spawn(&self) -> anyhow::Result<tokio::process::Child> {
        let mut command = tokio::process::Command::new(&self.binary);
        command
            .arg("--asset-root")
            .arg(&self.asset_root)
            .arg("--model-root")
            .arg(&self.model_root)
            .arg("--pipe")
            .arg(&self.pipe_name)
            .arg("--lifeline-pipe")
            .arg(&self.lifeline_pipe_name);
        if let Some(data_directory) = &self.data_directory {
            command.env("SCREENSEARCH_DATA_DIR", data_directory);
        }
        command
            .spawn()
            .context("spawn screensearch-model-worker process")
    }
}

fn resolve_worker_binary() -> anyhow::Result<PathBuf> {
    let current = std::env::current_exe().context("resolve daemon executable path")?;
    let worker_name = if cfg!(windows) {
        "screensearch-model-worker.exe"
    } else {
        "screensearch-model-worker"
    };
    let binary_dir = current
        .parent()
        .context("daemon executable has no parent directory")?;
    let worker = binary_dir.join(worker_name);
    if worker.is_file() {
        return Ok(worker);
    }
    let dev_worker = workspace_debug_worker_path(binary_dir, worker_name);
    if dev_worker.is_file() {
        return Ok(dev_worker);
    }
    anyhow::bail!(
        "model worker binary was not found beside the daemon; build it with `cargo build -p screensearch-model-worker`"
    )
}

/// Spawns the worker, accepts its lifeline, and waits for it to report readiness.
async fn start_worker(
    args: &WorkerSpawnArgs,
    ready_timeout: Duration,
) -> anyhow::Result<(tokio::process::Child, WorkerLifeline)> {
    let pending = create_worker_lifeline(&args.lifeline_pipe_name)
        .map_err(|error| anyhow::anyhow!("create worker lifeline: {error}"))?;
    let child = args.spawn()?;
    let lifeline = tokio::time::timeout(Duration::from_secs(10), pending.accept())
        .await
        .context("worker did not connect its lifeline in time")?
        .map_err(|error| anyhow::anyhow!("accept worker lifeline: {error}"))?;
    wait_for_model_worker_ready(ready_timeout).await?;
    Ok((child, lifeline))
}

/// Supervises the worker: detects exits, restarts within a bounded budget, and owns the
/// clean-shutdown kill. Returns `true` if the worker exhausted its restart budget, which
/// the caller surfaces as a loud daemon exit.
async fn worker_supervisor_loop(
    args: WorkerSpawnArgs,
    mut child: tokio::process::Child,
    mut lifeline: WorkerLifeline,
    shutdown_tx: watch::Sender<bool>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> bool {
    let mut policy = RestartPolicy::new();
    loop {
        tokio::select! {
            status = child.wait() => {
                warn!(?status, "model worker exited unexpectedly");
                match policy.on_exit(Instant::now()) {
                    RestartDecision::Restart { after } => {
                        tokio::time::sleep(after).await;
                        match start_worker(&args, Duration::from_secs(10)).await {
                            Ok((new_child, new_lifeline)) => {
                                child = new_child;
                                lifeline = new_lifeline;
                                info!("model worker restarted");
                            }
                            Err(error) => {
                                error!(error = %error, "failed to restart model worker; daemon will exit");
                                let _ = shutdown_tx.send(true);
                                return true;
                            }
                        }
                    }
                    RestartDecision::GiveUp => {
                        error!("model worker exceeded its restart budget; daemon will exit");
                        let _ = shutdown_tx.send(true);
                        return true;
                    }
                }
            }
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                    drop(lifeline);
                    return false;
                }
            }
        }
    }
}

async fn wait_for_shutdown(mut shutdown: watch::Receiver<bool>) {
    let _ = shutdown.wait_for(|signaled| *signaled).await;
}

fn workspace_debug_worker_path(binary_dir: &Path, worker_name: &str) -> PathBuf {
    for ancestor in binary_dir.ancestors() {
        if ancestor.file_name().and_then(|name| name.to_str()) == Some("debug")
            && ancestor
                .parent()
                .and_then(|parent| parent.file_name())
                .and_then(|name| name.to_str())
                == Some("target")
        {
            return ancestor.join(worker_name);
        }
        let candidate = ancestor.join("target").join("debug").join(worker_name);
        if candidate.is_file() {
            return candidate;
        }
    }
    binary_dir.join(worker_name)
}

fn data_directory() -> anyhow::Result<PathBuf> {
    if let Some(directory) = std::env::var_os("SCREENSEARCH_DATA_DIR") {
        return Ok(PathBuf::from(directory));
    }
    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .context("LOCALAPPDATA is unavailable; set SCREENSEARCH_DATA_DIR explicitly")?;
    Ok(PathBuf::from(local_app_data).join("ScreenSearchV2"))
}
