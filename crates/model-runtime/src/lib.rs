//! Local model providers and deterministic test doubles.

use std::{
    num::NonZeroU32,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use async_stream::try_stream;
use async_trait::async_trait;
use blake3::Hasher;
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{AddBos, LlamaChatMessage, LlamaModel, params::LlamaModelParams},
    sampling::LlamaSampler,
};
use screensearch_domain::{AssetRef, BoundingBox, OcrBlock};
use screensearch_ports::{EmbeddingEngine, OcrEngine, PortError, TextGenerator, TokenStream};
use tokio_stream::wrappers::ReceiverStream;
use tracing::info;

/// Bootstrap OCR provider used until the ONNX worker adapter is enabled.
#[derive(Default)]
pub struct FakeOcrEngine;

#[async_trait]
impl OcrEngine for FakeOcrEngine {
    fn model_id(&self) -> &'static str {
        "fake-ocr-v1"
    }

    async fn recognize(&self, asset: &AssetRef) -> Result<Vec<OcrBlock>, PortError> {
        Ok(vec![OcrBlock {
            reading_order: 0,
            bounds: BoundingBox {
                x: 0.05,
                y: 0.05,
                width: 0.9,
                height: 0.1,
            },
            text: format!(
                "ScreenSearch bootstrap capture {}",
                &asset.content_hash[..12.min(asset.content_hash.len())]
            ),
            confidence: Some(1.0),
            language: Some("en".to_owned()),
        }])
    }
}

/// Deterministic 384-dimensional embedding provider for integration tests.
#[derive(Default)]
pub struct FakeEmbeddingEngine;

#[async_trait]
impl EmbeddingEngine for FakeEmbeddingEngine {
    fn model_id(&self) -> &'static str {
        "fake-embedding-384-v1"
    }

    fn dimensions(&self) -> usize {
        384
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>, PortError> {
        if text.trim().is_empty() {
            return Err(PortError::InvalidData("cannot embed empty text".to_owned()));
        }

        let mut vector = Vec::with_capacity(self.dimensions());
        for block in 0..12_u32 {
            let mut hasher = Hasher::new();
            hasher.update(text.as_bytes());
            hasher.update(&block.to_le_bytes());
            vector.extend(
                hasher
                    .finalize()
                    .as_bytes()
                    .iter()
                    .map(|byte| (f32::from(*byte) / 127.5) - 1.0),
            );
        }
        let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
        for value in &mut vector {
            *value /= norm;
        }
        Ok(vector)
    }
}

/// Quantized ONNX sentence embedding provider backed by `fastembed`.
#[derive(Clone)]
pub struct FastEmbedEngine {
    model_root: PathBuf,
    model: Arc<Mutex<Option<TextEmbedding>>>,
}

impl FastEmbedEngine {
    /// Creates a lazy provider caching model files below `model_root`.
    pub fn new(model_root: impl Into<PathBuf>) -> Self {
        Self {
            model_root: model_root.into(),
            model: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait]
impl EmbeddingEngine for FastEmbedEngine {
    fn model_id(&self) -> &'static str {
        "fastembed-all-minilm-l6-v2-q-384-v1"
    }

    fn dimensions(&self) -> usize {
        384
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>, PortError> {
        if text.trim().is_empty() {
            return Err(PortError::InvalidData("cannot embed empty text".to_owned()));
        }
        let text = text.to_owned();
        let model_root = self.model_root.clone();
        let model = self.model.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = model
                .lock()
                .map_err(|_| PortError::Internal("embedding model lock was poisoned".to_owned()))?;
            if guard.is_none() {
                let options = TextInitOptions::new(EmbeddingModel::AllMiniLML6V2Q)
                    .with_cache_dir(model_root)
                    .with_show_download_progress(false)
                    .with_intra_threads(2)
                    .with_max_length(256);
                *guard = Some(TextEmbedding::try_new(options).map_err(|error| {
                    PortError::Unavailable(format!("load local MiniLM embedding model: {error}"))
                })?);
            }
            let embeddings = guard
                .as_mut()
                .expect("embedding model initialized")
                .embed([text], Some(1))
                .map_err(|error| PortError::Internal(format!("embed local text: {error}")))?;
            let mut vector = embeddings.into_iter().next().ok_or_else(|| {
                PortError::Internal("embedding model returned no vector".to_owned())
            })?;
            normalize(&mut vector)?;
            Ok(vector)
        })
        .await
        .map_err(|error| PortError::Internal(format!("embedding task failed: {error}")))?
    }
}

fn normalize(vector: &mut [f32]) -> Result<(), PortError> {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if !norm.is_finite() || norm <= f32::EPSILON {
        return Err(PortError::InvalidData(
            "embedding vector cannot be normalized".to_owned(),
        ));
    }
    for value in vector {
        *value /= norm;
    }
    Ok(())
}

/// Deterministic streaming generator used to prove cancellation and framing.
#[derive(Default)]
pub struct FakeTextGenerator;

#[async_trait]
impl TextGenerator for FakeTextGenerator {
    async fn generate(&self, prompt: String) -> Result<TokenStream, PortError> {
        let cited_capture = prompt
            .lines()
            .find(|line| line.starts_with('['))
            .and_then(|line| line.split_whitespace().next())
            .unwrap_or("[no-capture]")
            .to_owned();
        let response =
            format!("The indexed screen contains locally extracted text {cited_capture}.");
        Ok(Box::pin(try_stream! {
            for word in response.split_inclusive(' ') {
                yield word.to_owned();
            }
        }))
    }
}

/// Backstop wall-clock budget for a single generation request, measured from the
/// first generated token rather than from model load.
///
/// A code constant rather than a tunable: the spec adds no generation-deadline
/// environment variable, so this stays an internal safety limit. Arming it after
/// the (potentially slow, one-time) cold load means a long load can never silently
/// consume the budget and leave the answer empty.
const GENERATION_DEADLINE: Duration = Duration::from_secs(120);

/// Context window the generator evaluates with.
///
/// Large enough to hold a bounded retrieval prompt plus a reasoning model's
/// think-then-answer span. Exposed so the daemon stamps the same value into each
/// model's `context_tokens` metadata (keeping that claim truthful).
pub const GENERATION_CONTEXT_TOKENS: u32 = 4_096;

/// Hard cap on tokens emitted for a single generation request.
///
/// Headroom over the old 256 so a reasoning model can finish its `<think>` span and
/// still reach an answer; the wall-clock deadline remains the real bound.
const MAX_GENERATED_TOKENS: usize = 768;

/// Picks the CPU thread budget for llama.cpp inference.
///
/// Local generation is memory-bandwidth bound, so physical cores outperform SMT
/// logical threads. This replaces the previous hardcoded two-thread cap, which left
/// generation crawling (and appearing frozen) on multi-core machines.
fn cpu_thread_budget() -> i32 {
    i32::try_from(num_cpus::get_physical().max(1)).unwrap_or(1)
}

/// Local GGUF generator backed by llama.cpp.
#[derive(Clone)]
pub struct LlamaCppTextGenerator {
    model_path: PathBuf,
    model: Arc<Mutex<Option<LoadedLlamaModel>>>,
    loaded: Arc<AtomicBool>,
}

struct LoadedLlamaModel {
    backend: LlamaBackend,
    model: LlamaModel,
}

impl LlamaCppTextGenerator {
    /// Creates a lazy generator for an explicitly installed GGUF model.
    pub fn new(model_path: impl Into<PathBuf>) -> Self {
        Self {
            model_path: model_path.into(),
            model: Arc::new(Mutex::new(None)),
            loaded: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Returns whether this generator currently has a GGUF model resident in memory.
    ///
    /// Reads a lock-free flag so a health probe never blocks behind an in-flight
    /// generation that holds the model mutex for its entire token loop.
    pub fn is_loaded(&self) -> bool {
        self.loaded.load(Ordering::Acquire)
    }

    /// Drops any loaded GGUF model and releases its memory.
    pub fn unload(&self) -> Result<(), PortError> {
        *self
            .model
            .lock()
            .map_err(|_| PortError::Internal("generation lock was poisoned".to_owned()))? = None;
        self.loaded.store(false, Ordering::Release);
        Ok(())
    }
}

#[async_trait]
impl TextGenerator for LlamaCppTextGenerator {
    async fn generate(&self, prompt: String) -> Result<TokenStream, PortError> {
        if !self.model_path.is_file() {
            return Err(PortError::Unavailable(
                "local GGUF generator is not installed".to_owned(),
            ));
        }
        let model_path = self.model_path.clone();
        let model = self.model.clone();
        let loaded = self.loaded.clone();
        let (send, receive) = tokio::sync::mpsc::channel(32);
        tokio::task::spawn_blocking(move || {
            let result = generate_gguf(
                &model_path,
                &prompt,
                |piece| {
                    send.blocking_send(Ok(piece)).map_err(|_| {
                        PortError::Transient("generation consumer disconnected".to_owned())
                    })
                },
                &model,
                &loaded,
            );
            if let Err(error) = result {
                let _ = send.blocking_send(Err(error));
            }
        });
        Ok(Box::pin(ReceiverStream::new(receive)))
    }
}

/// Loads a GGUF model and its backend. Logs only content-free timing metadata.
fn load_model(model_path: &std::path::Path) -> Result<LoadedLlamaModel, PortError> {
    let load_started = Instant::now();
    let backend = LlamaBackend::init()
        .map_err(|error| PortError::Internal(format!("initialize llama.cpp: {error}")))?;
    let model = LlamaModel::load_from_file(&backend, model_path, &LlamaModelParams::default())
        .map_err(|error| PortError::Unavailable(format!("load local GGUF model: {error}")))?;
    info!(
        load_ms = u64::try_from(load_started.elapsed().as_millis()).unwrap_or(u64::MAX),
        "loaded local GGUF generation model"
    );
    Ok(LoadedLlamaModel { backend, model })
}

/// Wraps the assembled prompt in the model's own chat template so instruct and
/// reasoning models receive the role markers and trailing assistant tag they expect
/// (without it, a chat model treats the prompt as raw text to continue and a reasoning
/// model can spend its whole budget mid-thought without ever answering).
///
/// Falls back to the raw prompt for base models that ship no template, and on any
/// templating error, so generation never fails for want of a template.
fn apply_chat_format(model: &LlamaModel, prompt: &str) -> String {
    let Ok(template) = model.chat_template(None) else {
        return prompt.to_owned();
    };
    let Ok(message) = LlamaChatMessage::new("user".to_owned(), prompt.to_owned()) else {
        return prompt.to_owned();
    };
    model
        .apply_chat_template(&template, &[message], true)
        .unwrap_or_else(|_| prompt.to_owned())
}

fn generate_gguf(
    model_path: &std::path::Path,
    prompt: &str,
    mut emit: impl FnMut(String) -> Result<(), PortError>,
    model_cache: &Mutex<Option<LoadedLlamaModel>>,
    loaded: &AtomicBool,
) -> Result<(), PortError> {
    let mut cached = model_cache
        .lock()
        .map_err(|_| PortError::Internal("generation lock was poisoned".to_owned()))?;
    if cached.is_none() {
        *cached = Some(load_model(model_path)?);
    }
    loaded.store(true, Ordering::Release);
    let cached = cached.as_mut().expect("llama model initialized");
    let threads = cpu_thread_budget();
    let context_parameters = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(GENERATION_CONTEXT_TOKENS))
        .with_n_threads(threads)
        .with_n_threads_batch(threads);
    let mut context = cached
        .model
        .new_context(&cached.backend, context_parameters)
        .map_err(|error| PortError::Internal(format!("create llama.cpp context: {error}")))?;
    let formatted_prompt = apply_chat_format(&cached.model, prompt);
    let tokens = cached
        .model
        .str_to_token(&formatted_prompt, AddBos::Always)
        .map_err(|error| PortError::InvalidData(format!("tokenize generation prompt: {error}")))?;
    let maximum_prompt_tokens = 1_792_usize;
    if tokens.is_empty() || tokens.len() > maximum_prompt_tokens {
        return Err(PortError::InvalidData(format!(
            "generation prompt contains {} tokens; maximum is {maximum_prompt_tokens}",
            tokens.len()
        )));
    }
    let prompt_token_count = tokens.len();

    let mut batch = LlamaBatch::new(2_048, 1);
    let last = tokens.len() - 1;
    for (position, token) in tokens.into_iter().enumerate() {
        batch
            .add(
                token,
                i32::try_from(position).map_err(|_| {
                    PortError::InvalidData("generation prompt is too long".to_owned())
                })?,
                &[0],
                position == last,
            )
            .map_err(|error| PortError::Internal(format!("build llama.cpp batch: {error}")))?;
    }
    let prompt_decode_started = Instant::now();
    context
        .decode(&mut batch)
        .map_err(|error| PortError::Internal(format!("evaluate generation prompt: {error}")))?;
    let prompt_decode_ms =
        u64::try_from(prompt_decode_started.elapsed().as_millis()).unwrap_or(u64::MAX);

    // Arm the wall-clock budget only now that the model is resident and the prompt
    // is evaluated, so a slow cold load never eats into generation time.
    let deadline = Instant::now() + GENERATION_DEADLINE;
    let generation_started = Instant::now();
    let mut sampler = LlamaSampler::greedy();
    let mut decoder = encoding_rs::UTF_8.new_decoder();
    let mut generated_tokens = 0_usize;
    let mut stop_reason = "token_cap";
    for position in (batch.n_tokens()..).take(MAX_GENERATED_TOKENS) {
        if should_stop_generation(deadline, Instant::now()) {
            stop_reason = "deadline";
            break;
        }
        let token = sampler.sample(&context, batch.n_tokens() - 1);
        sampler.accept(token);
        if cached.model.is_eog_token(token) {
            stop_reason = "eog";
            break;
        }
        let piece = cached
            .model
            .token_to_piece(token, &mut decoder, true, None)
            .map_err(|error| PortError::Internal(format!("decode generated token: {error}")))?;
        if !piece.is_empty() {
            emit(piece)?;
        }
        generated_tokens += 1;
        batch.clear();
        batch
            .add(token, position, &[0], true)
            .map_err(|error| PortError::Internal(format!("build generation batch: {error}")))?;
        context
            .decode(&mut batch)
            .map_err(|error| PortError::Internal(format!("evaluate generated token: {error}")))?;
    }
    info!(
        threads,
        prompt_tokens = prompt_token_count,
        prompt_decode_ms,
        generated_tokens,
        generate_ms = u64::try_from(generation_started.elapsed().as_millis()).unwrap_or(u64::MAX),
        stop_reason,
        "completed local generation"
    );
    Ok(())
}

/// Returns whether token generation should stop because its wall-clock deadline elapsed.
///
/// Factored out of the token loop so the deadline policy is unit-testable without a
/// live GGUF model.
fn should_stop_generation(deadline: Instant, now: Instant) -> bool {
    now >= deadline
}

/// Explicit marker for the still-unimplemented generic ONNX vision-provider boundary.
pub struct OnnxRuntimeProvider;

impl OnnxRuntimeProvider {
    /// Returns an explicit error until generic vision model manifests are introduced.
    pub fn unavailable() -> PortError {
        PortError::Unavailable("generic ONNX vision provider is not configured".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use screensearch_ports::EmbeddingEngine;

    use super::{FakeEmbeddingEngine, should_stop_generation};

    #[tokio::test]
    async fn fake_embeddings_are_normalized_and_fixed_width() {
        let vector = FakeEmbeddingEngine.embed("ScreenSearch").await.unwrap();
        let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();

        assert_eq!(vector.len(), 384);
        assert!((norm - 1.0).abs() < 0.0001);
    }

    #[test]
    fn generation_stops_only_once_its_deadline_elapses() {
        let start = Instant::now();
        let deadline = start + Duration::from_secs(120);

        assert!(!should_stop_generation(deadline, start));
        assert!(!should_stop_generation(
            deadline,
            start + Duration::from_secs(119)
        ));
        assert!(should_stop_generation(deadline, deadline));
        assert!(should_stop_generation(
            deadline,
            start + Duration::from_secs(121)
        ));
    }
}
