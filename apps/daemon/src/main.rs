//! Persistent ScreenSearch V2 daemon and named-pipe endpoint.

use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::Context;
use futures::{StreamExt, stream};
use screensearch_application::{
    AnalysisService, CapturePolicy, CapturePolicyConfig, IngestService, SearchService,
};
use screensearch_domain::{
    ArchiveSettings, CaptureDisposition, CaptureId, DeleteCaptures, SearchEvent, SearchMatchKind,
    StorageMetrics,
};
use screensearch_ipc::{
    IpcError, RequestHandler, ResponseStream,
    transport::{DEFAULT_PIPE_NAME, serve},
    v1::{
        ArchiveSettingsResponse, CaptureAssetResponse, CaptureResponse, Citation,
        DeleteCapturesResponse, ErrorResponse, HealthResponse, NormalizedRect, ProcessJobsResponse,
        ResponseEnvelope, SearchCompleted, SearchEvent as IpcSearchEvent, SetCapturePausedResponse,
        Token, UpdateArchiveSettingsResponse, request_envelope, response_envelope, search_event,
    },
};
use screensearch_model_runtime::{FastEmbedEngine, LlamaCppTextGenerator};
use screensearch_persistence::{FileAssetStore, LibSqlArchive};
use screensearch_ports::ArchiveRepository;
use screensearch_windows::{WindowsGraphicsCaptureSource, WindowsOcrEngine};
use tokio::sync::watch;
use tracing::{info, warn};

struct DaemonHandler {
    ingest: Arc<IngestService>,
    analysis: Arc<AnalysisService>,
    search: Arc<SearchService>,
    repository: Arc<LibSqlArchive>,
    assets: Arc<FileAssetStore>,
    capture_policy: Arc<CapturePolicy>,
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
        SearchEvent::Completed { citation_count } => (
            search_event::Event::Completed(SearchCompleted {
                citation_count: u32::try_from(citation_count).unwrap_or(u32::MAX),
            }),
            true,
        ),
    }
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
    let embeddings = Arc::new(FastEmbedEngine::new(data_directory.join("models")));
    let generator = Arc::new(LlamaCppTextGenerator::new(
        data_directory.join("models/generator/model.gguf"),
    ));
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
        Arc::new(WindowsOcrEngine::new(asset_root)),
        embeddings.clone(),
        "daemon-windows-worker",
    ));
    let handler = Arc::new(DaemonHandler {
        ingest: ingest.clone(),
        analysis: analysis.clone(),
        search: Arc::new(SearchService::new(
            repository.clone(),
            embeddings,
            generator,
        )),
        repository,
        assets,
        capture_policy,
    });

    info!(pipe = DEFAULT_PIPE_NAME, "daemon ready");
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let capture_task = tokio::spawn(capture_loop(ingest, shutdown_rx.clone()));
    let analysis_task = tokio::spawn(analysis_loop(analysis, shutdown_rx));
    let maintenance_task = tokio::spawn(maintenance_loop(
        handler.ingest.clone(),
        shutdown_tx.subscribe(),
    ));
    tokio::select! {
        result = serve(DEFAULT_PIPE_NAME, handler) => result.context("serve named pipe")?,
        result = tokio::signal::ctrl_c() => result.context("wait for shutdown signal")?,
    }
    let _ = shutdown_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(3), capture_task).await;
    let _ = tokio::time::timeout(Duration::from_secs(3), analysis_task).await;
    let _ = tokio::time::timeout(Duration::from_secs(3), maintenance_task).await;
    info!("daemon stopped");
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

fn data_directory() -> anyhow::Result<PathBuf> {
    if let Some(directory) = std::env::var_os("SCREENSEARCH_DATA_DIR") {
        return Ok(PathBuf::from(directory));
    }
    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .context("LOCALAPPDATA is unavailable; set SCREENSEARCH_DATA_DIR explicitly")?;
    Ok(PathBuf::from(local_app_data).join("ScreenSearchV2"))
}
