//! Local model providers and deterministic test doubles.

use std::{
    num::NonZeroU32,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use async_stream::try_stream;
use async_trait::async_trait;
use blake3::Hasher;
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{AddBos, LlamaModel, params::LlamaModelParams},
    sampling::LlamaSampler,
};
use screensearch_domain::{AssetRef, BoundingBox, OcrBlock};
use screensearch_ports::{EmbeddingEngine, OcrEngine, PortError, TextGenerator, TokenStream};
use tokio_stream::wrappers::ReceiverStream;

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

/// Local GGUF generator backed by llama.cpp.
#[derive(Clone, Debug)]
pub struct LlamaCppTextGenerator {
    model_path: PathBuf,
    generation_gate: Arc<Mutex<()>>,
}

impl LlamaCppTextGenerator {
    /// Creates a lazy generator for an explicitly installed GGUF model.
    pub fn new(model_path: impl Into<PathBuf>) -> Self {
        Self {
            model_path: model_path.into(),
            generation_gate: Arc::new(Mutex::new(())),
        }
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
        let generation_gate = self.generation_gate.clone();
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
                &generation_gate,
            );
            if let Err(error) = result {
                let _ = send.blocking_send(Err(error));
            }
        });
        Ok(Box::pin(ReceiverStream::new(receive)))
    }
}

fn generate_gguf(
    model_path: &std::path::Path,
    prompt: &str,
    mut emit: impl FnMut(String) -> Result<(), PortError>,
    generation_gate: &Mutex<()>,
) -> Result<(), PortError> {
    let _guard = generation_gate
        .lock()
        .map_err(|_| PortError::Internal("generation lock was poisoned".to_owned()))?;
    let backend = LlamaBackend::init()
        .map_err(|error| PortError::Internal(format!("initialize llama.cpp: {error}")))?;
    let model = LlamaModel::load_from_file(&backend, model_path, &LlamaModelParams::default())
        .map_err(|error| PortError::Unavailable(format!("load local GGUF model: {error}")))?;
    let context_parameters = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(2_048))
        .with_n_threads(2)
        .with_n_threads_batch(2);
    let mut context = model
        .new_context(&backend, context_parameters)
        .map_err(|error| PortError::Internal(format!("create llama.cpp context: {error}")))?;
    let tokens = model
        .str_to_token(prompt, AddBos::Always)
        .map_err(|error| PortError::InvalidData(format!("tokenize generation prompt: {error}")))?;
    let maximum_prompt_tokens = 1_792_usize;
    if tokens.is_empty() || tokens.len() > maximum_prompt_tokens {
        return Err(PortError::InvalidData(format!(
            "generation prompt contains {} tokens; maximum is {maximum_prompt_tokens}",
            tokens.len()
        )));
    }

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
    context
        .decode(&mut batch)
        .map_err(|error| PortError::Internal(format!("evaluate generation prompt: {error}")))?;

    let mut sampler = LlamaSampler::greedy();
    let mut decoder = encoding_rs::UTF_8.new_decoder();
    for position in (batch.n_tokens()..).take(256) {
        let token = sampler.sample(&context, batch.n_tokens() - 1);
        sampler.accept(token);
        if model.is_eog_token(token) {
            break;
        }
        let piece = model
            .token_to_piece(token, &mut decoder, true, None)
            .map_err(|error| PortError::Internal(format!("decode generated token: {error}")))?;
        if !piece.is_empty() {
            emit(piece)?;
        }
        batch.clear();
        batch
            .add(token, position, &[0], true)
            .map_err(|error| PortError::Internal(format!("build generation batch: {error}")))?;
        context
            .decode(&mut batch)
            .map_err(|error| PortError::Internal(format!("evaluate generated token: {error}")))?;
    }
    Ok(())
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
    use screensearch_ports::EmbeddingEngine;

    use super::FakeEmbeddingEngine;

    #[tokio::test]
    async fn fake_embeddings_are_normalized_and_fixed_width() {
        let vector = FakeEmbeddingEngine.embed("ScreenSearch").await.unwrap();
        let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();

        assert_eq!(vector.len(), 384);
        assert!((norm - 1.0).abs() < 0.0001);
    }
}
