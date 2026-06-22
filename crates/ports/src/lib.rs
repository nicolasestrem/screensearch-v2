//! Dependency-inversion ports implemented by capture, storage, model, and automation adapters.

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
use screensearch_domain::{
    AnalysisJob, AnalysisResult, ArchiveSettings, AssetCleanupTask, AssetRef, CaptureDisposition,
    CaptureId, CapturedFrame, DeleteCaptures, DeletionSummary, GenerationModel, NewCapture,
    OcrBlock, QueueMetrics, SearchHit, StorageMetrics,
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

    /// Removes an unreferenced asset idempotently.
    async fn delete(&self, asset: &AssetRef) -> Result<(), PortError>;
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

    /// Returns content-free queue health for backpressure and diagnostics.
    async fn queue_metrics(&self) -> Result<QueueMetrics, PortError>;

    /// Loads versioned user-controlled archive policy.
    async fn archive_settings(&self) -> Result<ArchiveSettings, PortError>;

    /// Replaces archive policy and exclusion rules atomically.
    async fn update_archive_settings(&self, settings: ArchiveSettings) -> Result<(), PortError>;

    /// Returns content-free archive size and indexing measurements.
    async fn storage_metrics(&self) -> Result<StorageMetrics, PortError>;

    /// Applies age and asset-budget retention to eligible captures.
    async fn apply_retention(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<DeletionSummary, PortError>;

    /// Deletes explicitly selected captures without touching active analysis leases.
    async fn delete_captures(&self, request: DeleteCaptures) -> Result<DeletionSummary, PortError>;

    /// Returns the oldest durable unreferenced-asset cleanup task.
    async fn claim_asset_cleanup(&self) -> Result<Option<AssetCleanupTask>, PortError>;

    /// Completes an asset cleanup after the filesystem delete succeeds.
    async fn complete_asset_cleanup(&self, content_hash: &str) -> Result<(), PortError>;

    /// Records a bounded cleanup failure for later retry.
    async fn fail_asset_cleanup(&self, content_hash: &str, reason: &str) -> Result<(), PortError>;

    /// Runs lexical and vector retrieval and fuses their ranks.
    async fn hybrid_search(
        &self,
        query: &str,
        embedding: &[f32],
        model_id: &str,
        limit: usize,
    ) -> Result<Vec<SearchHit>, PortError>;

    /// Lists known generation models and their active state.
    async fn generation_models(&self) -> Result<Vec<GenerationModel>, PortError>;

    /// Inserts or replaces one generation model catalog entry.
    async fn upsert_generation_model(&self, model: GenerationModel) -> Result<(), PortError>;

    /// Marks one generation model active and deactivates all others.
    async fn select_generation_model(&self, model_id: &str) -> Result<(), PortError>;

    /// Clears the active generation model selection.
    async fn clear_active_generation_model(&self) -> Result<(), PortError>;

    /// Returns the active generation model, if one is selected.
    async fn active_generation_model(&self) -> Result<Option<GenerationModel>, PortError>;

    /// Deletes an inactive generation model catalog entry.
    async fn delete_generation_model(
        &self,
        model_id: &str,
    ) -> Result<Option<GenerationModel>, PortError>;
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
