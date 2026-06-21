//! Dependency-inversion ports implemented by capture, storage, model, and automation adapters.

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
use screensearch_domain::{
    AnalysisJob, AnalysisResult, AssetRef, CaptureDisposition, CaptureId, CapturedFrame,
    NewCapture, OcrBlock, SearchHit,
};
use thiserror::Error;

/// Stream returned by a local text-generation provider.
pub type TokenStream = Pin<Box<dyn Stream<Item = Result<String, PortError>> + Send>>;

/// Captures a privacy-filtered screen frame.
#[async_trait]
pub trait CaptureSource: Send + Sync {
    /// Captures the next eligible frame.
    async fn capture(&self) -> Result<CapturedFrame, PortError>;
}

/// Stores immutable capture payloads outside the relational database.
#[async_trait]
pub trait AssetStore: Send + Sync {
    /// Writes bytes idempotently and returns their content-addressed location.
    async fn put(&self, bytes: &[u8], media_type: &str) -> Result<AssetRef, PortError>;
}

/// Transactional archive and hybrid-search boundary.
#[async_trait]
pub trait ArchiveRepository: Send + Sync {
    /// Applies idempotent schema migrations.
    async fn migrate(&self) -> Result<(), PortError>;

    /// Inserts capture metadata and its job in one transaction, or reports a duplicate.
    async fn enqueue_capture(&self, capture: NewCapture) -> Result<CaptureDisposition, PortError>;

    /// Claims one available job using a bounded lease.
    async fn claim_job(&self, worker_id: &str) -> Result<Option<AnalysisJob>, PortError>;

    /// Commits OCR, chunks, embeddings, outbox events, and job completion atomically.
    async fn complete_analysis(&self, result: AnalysisResult) -> Result<(), PortError>;

    /// Records a failed attempt and schedules a retry or dead-letters the job.
    async fn fail_job(&self, job: &AnalysisJob, reason: &str) -> Result<(), PortError>;

    /// Resolves an immutable asset by its authorized capture identifier.
    async fn capture_asset(&self, capture_id: CaptureId) -> Result<Option<AssetRef>, PortError>;

    /// Runs lexical and vector retrieval and fuses their ranks.
    async fn hybrid_search(
        &self,
        query: &str,
        embedding: &[f32],
        model_id: &str,
        limit: usize,
    ) -> Result<Vec<SearchHit>, PortError>;
}

/// Performs OCR for one immutable asset.
#[async_trait]
pub trait OcrEngine: Send + Sync {
    /// Stable model identifier stored with derived data.
    fn model_id(&self) -> &'static str;

    /// Recognizes ordered text blocks.
    async fn recognize(&self, asset: &AssetRef) -> Result<Vec<OcrBlock>, PortError>;
}

/// Produces vectors for documents and search queries.
#[async_trait]
pub trait EmbeddingEngine: Send + Sync {
    /// Stable model identifier stored with every vector.
    fn model_id(&self) -> &'static str;

    /// Fixed vector dimension.
    fn dimensions(&self) -> usize;

    /// Embeds normalized text.
    async fn embed(&self, text: &str) -> Result<Vec<f32>, PortError>;
}

/// Streams locally generated text from assembled evidence.
#[async_trait]
pub trait TextGenerator: Send + Sync {
    /// Streams an answer for an already policy-safe prompt.
    async fn generate(&self, prompt: String) -> Result<TokenStream, PortError>;
}

/// A validated, explicitly approved automation action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovedAutomationAction {
    /// Identifier of the approval record.
    pub approval_id: String,
    /// Expected foreground window title.
    pub expected_window: String,
    /// Deterministic action description.
    pub action: String,
}

/// Executes a guarded OS automation action.
#[async_trait]
pub trait AutomationExecutor: Send + Sync {
    /// Executes only after approval, foreground, and abort checks pass.
    async fn execute(&self, action: &ApprovedAutomationAction) -> Result<(), PortError>;
}

/// Errors crossing an adapter boundary.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PortError {
    /// A configured resource could not be reached.
    #[error("resource unavailable: {0}")]
    Unavailable(String),
    /// Input or persisted data violated an adapter invariant.
    #[error("invalid data: {0}")]
    InvalidData(String),
    /// A transient operation may be retried.
    #[error("transient failure: {0}")]
    Transient(String),
    /// A policy gate rejected the operation.
    #[error("operation denied: {0}")]
    Denied(String),
    /// An unexpected adapter failure occurred.
    #[error("adapter failure: {0}")]
    Internal(String),
}
