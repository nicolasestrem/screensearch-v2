//! Dependency-inversion ports implemented by capture, storage, model, and automation adapters.

use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use async_trait::async_trait;
use futures::Stream;
use screensearch_domain::{
    AnalysisJob, AnalysisResult, ArchiveSettings, AssetCleanupTask, AssetRef, AutomationAction,
    AutomationFailureCode, AutomationRun, AutomationRunId, AutomationRunStatus, AutomationSettings,
    AutomationTarget, CaptureDisposition, CaptureId, CapturedFrame, DeleteCaptures,
    DeletionSummary, GenerationModel, NewCapture, OcrBlock, QueueMetrics, SearchFilters, SearchHit,
    StorageMetrics,
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
        filters: &SearchFilters,
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

/// Result of atomically claiming a one-shot automation approval.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AutomationClaimOutcome {
    /// Exact, live approval was transitioned to running.
    Claimed(AutomationRun),
    /// Approval does not exist or was already consumed.
    Missing,
    /// Approval elapsed and was transitioned to expired.
    Expired,
    /// Approval exists but its canonical digest differs.
    PlanMismatch,
}

/// Durable default-off settings and content-free automation run ledger.
#[async_trait]
pub trait AutomationRepository: Send + Sync {
    /// Loads daemon-owned automation enablement.
    async fn automation_settings(&self) -> Result<AutomationSettings, PortError>;

    /// Replaces daemon-owned automation enablement.
    async fn update_automation_settings(
        &self,
        settings: AutomationSettings,
    ) -> Result<(), PortError>;

    /// Persists one exact digest approval.
    async fn create_automation_approval(&self, run: AutomationRun) -> Result<(), PortError>;

    /// Atomically validates and consumes one approval.
    async fn claim_automation_run(
        &self,
        id: AutomationRunId,
        plan_digest: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<AutomationClaimOutcome, PortError>;

    /// Writes a terminal status and optional content-free failure code.
    async fn finish_automation_run(
        &self,
        id: AutomationRunId,
        status: AutomationRunStatus,
        failure_code: Option<AutomationFailureCode>,
        finished_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), PortError>;

    /// Returns one content-free ledger record.
    async fn automation_run(&self, id: AutomationRunId)
    -> Result<Option<AutomationRun>, PortError>;

    /// Converts orphaned running rows to aborted during daemon startup.
    async fn recover_automation_runs(
        &self,
        recovered_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, PortError>;
}

/// Shared cancellation signal for one in-flight native automation action.
#[derive(Clone, Debug, Default)]
pub struct AutomationAbortSignal {
    cancelled: Arc<AtomicBool>,
}

impl AutomationAbortSignal {
    /// Creates a clear cancellation signal.
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks the action cancelled. Native adapters must fail closed before emitting input.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Returns true once the action has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// Native platform observations and one typed action emission.
#[async_trait]
pub trait AutomationPlatform: Send + Sync {
    /// Captures the exact current foreground HWND/PID/executable identity.
    async fn foreground_target(&self) -> Result<AutomationTarget, PortError>;

    /// Returns true only when Windows positively reports the interactive session unlocked.
    async fn session_is_unlocked(&self) -> Result<bool, PortError>;

    /// Executes exactly one already validated action against the approved target.
    async fn execute_action(
        &self,
        target: &AutomationTarget,
        action: &AutomationAction,
        abort_signal: AutomationAbortSignal,
    ) -> Result<(), PortError>;
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
    /// Guarded automation failed with a stable content-free category.
    #[error("automation denied: {0}")]
    Automation(screensearch_domain::AutomationFailureCode),
    /// An unexpected adapter failure occurred.
    #[error("adapter failure: {0}")]
    Internal(String),
}
