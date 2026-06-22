//! Supervised model-worker process entry point.

use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use anyhow::Context;
use futures::{StreamExt, stream};
use screensearch_domain::AssetRef;
use screensearch_ipc::{
    IpcError, RequestHandler, ResponseStream,
    transport::{DEFAULT_WORKER_PIPE_NAME, serve, watch_worker_lifeline},
    v1::{
        ErrorResponse, NormalizedRect, ResponseEnvelope, Token, WorkerEmbeddingResponse,
        WorkerGenerationCompleted, WorkerGenerationEvent, WorkerHealthResponse, WorkerOcrBlock,
        WorkerOcrResponse, WorkerUnloadResponse, request_envelope, response_envelope,
        worker_generation_event,
    },
};
use screensearch_model_runtime::{FastEmbedEngine, llama_sidecar::PreferredLlamaTextGenerator};
use screensearch_ports::{EmbeddingEngine, OcrEngine, TextGenerator};
use screensearch_windows::WindowsOcrEngine;
use tokio::sync::Mutex;
use tracing::{info, warn};

struct WorkerHandler {
    model_root: PathBuf,
    sidecar_root: PathBuf,
    ocr: Arc<WindowsOcrEngine>,
    embeddings: Arc<FastEmbedEngine>,
    generation: Mutex<Option<CachedGenerator>>,
}

struct CachedGenerator {
    model_id: String,
    relative_path: String,
    generator: PreferredLlamaTextGenerator,
}

#[async_trait::async_trait]
impl RequestHandler for WorkerHandler {
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
            request_envelope::Body::WorkerHealth(_) => {
                let generation = self.generation.lock().await;
                let generation_loaded = generation
                    .as_ref()
                    .is_some_and(|cached| cached.generator.is_loaded());
                let active_generation_model_id = generation
                    .as_ref()
                    .map(|cached| cached.model_id.clone())
                    .unwrap_or_default();
                Ok(single_response(ResponseEnvelope {
                    request_id,
                    terminal: true,
                    body: Some(response_envelope::Body::WorkerHealth(
                        WorkerHealthResponse {
                            status: "ready".to_owned(),
                            active_generation_model_id,
                            generation_loaded,
                        },
                    )),
                }))
            }
            request_envelope::Body::WorkerOcr(command) => {
                let asset = AssetRef {
                    content_hash: String::new(),
                    relative_path: command.asset_relative_path,
                    media_type: command.media_type,
                    byte_length: 0,
                };
                let blocks = self
                    .ocr
                    .recognize(&asset)
                    .await
                    .map_err(|error| IpcError::Handler(error.to_string()))?;
                Ok(single_response(ResponseEnvelope {
                    request_id,
                    terminal: true,
                    body: Some(response_envelope::Body::WorkerOcr(WorkerOcrResponse {
                        model_id: self.ocr.model_id().to_owned(),
                        blocks: blocks
                            .into_iter()
                            .map(|block| WorkerOcrBlock {
                                reading_order: block.reading_order,
                                bounds: Some(NormalizedRect {
                                    x: block.bounds.x,
                                    y: block.bounds.y,
                                    width: block.bounds.width,
                                    height: block.bounds.height,
                                }),
                                text: block.text,
                                confidence: block.confidence,
                                language: block.language.unwrap_or_default(),
                            })
                            .collect(),
                    })),
                }))
            }
            request_envelope::Body::WorkerEmbedding(command) => {
                let vector = self
                    .embeddings
                    .embed(&command.text)
                    .await
                    .map_err(|error| IpcError::Handler(error.to_string()))?;
                Ok(single_response(ResponseEnvelope {
                    request_id,
                    terminal: true,
                    body: Some(response_envelope::Body::WorkerEmbedding(
                        WorkerEmbeddingResponse {
                            model_id: self.embeddings.model_id().to_owned(),
                            dimensions: u32::try_from(self.embeddings.dimensions())
                                .unwrap_or(u32::MAX),
                            vector,
                        },
                    )),
                }))
            }
            request_envelope::Body::WorkerGeneration(command) => {
                validate_model_relative_path(&command.model_relative_path)?;
                let generator = {
                    let mut generation = self.generation.lock().await;
                    let reload = generation.as_ref().is_none_or(|cached| {
                        cached.relative_path != command.model_relative_path
                            || cached.model_id != command.model_id
                    });
                    if reload {
                        *generation = Some(CachedGenerator {
                            model_id: command.model_id.clone(),
                            relative_path: command.model_relative_path.clone(),
                            generator: PreferredLlamaTextGenerator::new(
                                self.model_root.join(&command.model_relative_path),
                                self.sidecar_root.clone(),
                            ),
                        });
                    }
                    generation
                        .as_ref()
                        .expect("generation cache initialized")
                        .generator
                        .clone()
                };
                let tokens = generator
                    .generate(command.prompt)
                    .await
                    .map_err(|error| IpcError::Handler(error.to_string()))?;
                let stream_id = request_id.clone();
                let terminal_id = request_id;
                let responses = tokens.map(move |token| {
                    token
                        .map(|text| ResponseEnvelope {
                            request_id: stream_id.clone(),
                            terminal: false,
                            body: Some(response_envelope::Body::WorkerGeneration(
                                WorkerGenerationEvent {
                                    event: Some(worker_generation_event::Event::Token(Token {
                                        text,
                                    })),
                                },
                            )),
                        })
                        .map_err(|error| IpcError::Handler(error.to_string()))
                });
                let completed = stream::once(async move {
                    Ok(ResponseEnvelope {
                        request_id: terminal_id,
                        terminal: true,
                        body: Some(response_envelope::Body::WorkerGeneration(
                            WorkerGenerationEvent {
                                event: Some(worker_generation_event::Event::Completed(
                                    WorkerGenerationCompleted {
                                        status: "answered".to_owned(),
                                    },
                                )),
                            },
                        )),
                    })
                });
                Ok(Box::pin(responses.chain(completed)))
            }
            request_envelope::Body::WorkerUnload(_) => {
                let mut generation = self.generation.lock().await;
                if let Some(cached) = generation.as_ref() {
                    cached
                        .generator
                        .unload()
                        .map_err(|error| IpcError::Handler(error.to_string()))?;
                }
                *generation = None;
                Ok(single_response(ResponseEnvelope {
                    request_id,
                    terminal: true,
                    body: Some(response_envelope::Body::WorkerUnload(
                        WorkerUnloadResponse { unloaded: true },
                    )),
                }))
            }
            _ => Ok(single_response(error_response(
                request_id,
                "wrong_endpoint",
                "daemon request was sent to the model-worker endpoint",
                false,
            ))),
        }
    }
}

fn validate_model_relative_path(relative_path: &str) -> Result<(), IpcError> {
    let path = Path::new(relative_path);
    if relative_path.trim().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(IpcError::Handler(
            "model path escapes model root".to_owned(),
        ));
    }
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "screensearch_model_worker=info,screensearch_model_runtime=info".into()
            }),
        )
        .try_init()
        .map_err(|error| anyhow::anyhow!("initialize model-worker tracing: {error}"))?;

    if let Some(model_path) = benchmark_model_arg() {
        return benchmark_model(model_path).await;
    }

    let WorkerConfig {
        asset_root,
        model_root,
        sidecar_root,
        pipe_name,
        lifeline_pipe,
    } = worker_config()?;
    if let Some(lifeline_pipe) = lifeline_pipe {
        tokio::spawn(async move {
            if let Err(error) = watch_worker_lifeline(&lifeline_pipe).await {
                warn!(%error, "model-worker lifeline watch failed");
            }
            info!("daemon lifeline closed; model worker exiting");
            std::process::exit(0);
        });
    }
    let handler = Arc::new(WorkerHandler {
        model_root: model_root.clone(),
        sidecar_root,
        ocr: Arc::new(WindowsOcrEngine::new(asset_root)),
        embeddings: Arc::new(FastEmbedEngine::new(model_root)),
        generation: Mutex::new(None),
    });
    info!(pipe = %pipe_name, "model worker ready");
    serve(&pipe_name, handler)
        .await
        .context("serve model-worker named pipe")?;
    Ok(())
}

async fn benchmark_model(model_path: PathBuf) -> anyhow::Result<()> {
    let sidecar_root = data_directory()?.join("sidecar").join("llama");
    let generator = PreferredLlamaTextGenerator::new(&model_path, sidecar_root);
    let prompt = "Answer only from the supplied local captures. Cite capture identifiers in brackets.\n\n[00000000-0000-7000-8000-000000000001] The ScreenSearch benchmark phrase is cobalt window.\n\nQuestion: What benchmark phrase was visible?";
    let started = Instant::now();
    let mut stream = generator
        .generate(prompt.to_owned())
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let mut token_pieces = 0_u64;
    while let Some(piece) = stream.next().await {
        piece.map_err(|error| anyhow::anyhow!(error.to_string()))?;
        token_pieces = token_pieces.saturating_add(1);
    }
    println!(
        "model_benchmark\tpath={}\tduration_ms={}\ttoken_pieces={}\tstatus=ok",
        model_path.display(),
        started.elapsed().as_millis(),
        token_pieces
    );
    Ok(())
}

fn benchmark_model_arg() -> Option<PathBuf> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--benchmark-model" {
            return args.next().map(PathBuf::from);
        }
    }
    None
}

struct WorkerConfig {
    asset_root: PathBuf,
    model_root: PathBuf,
    sidecar_root: PathBuf,
    pipe_name: String,
    lifeline_pipe: Option<String>,
}

fn worker_config() -> anyhow::Result<WorkerConfig> {
    let mut asset_root = None;
    let mut model_root = None;
    let mut pipe_name = None;
    let mut lifeline_pipe = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--asset-root" => asset_root = args.next().map(PathBuf::from),
            "--model-root" => model_root = args.next().map(PathBuf::from),
            "--pipe" => pipe_name = args.next(),
            "--lifeline-pipe" => lifeline_pipe = args.next(),
            _ => {}
        }
    }
    let data_root = data_directory()?;
    Ok(WorkerConfig {
        asset_root: asset_root.unwrap_or_else(|| data_root.join("assets")),
        model_root: model_root.unwrap_or_else(|| data_root.join("models")),
        sidecar_root: data_root.join("sidecar").join("llama"),
        pipe_name: pipe_name.unwrap_or_else(|| DEFAULT_WORKER_PIPE_NAME.to_owned()),
        lifeline_pipe,
    })
}

fn data_directory() -> anyhow::Result<PathBuf> {
    if let Some(directory) = std::env::var_os("SCREENSEARCH_DATA_DIR") {
        return Ok(PathBuf::from(directory));
    }
    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .context("LOCALAPPDATA is unavailable; set SCREENSEARCH_DATA_DIR explicitly")?;
    Ok(PathBuf::from(local_app_data).join("ScreenSearchV2"))
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
