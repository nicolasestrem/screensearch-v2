//! Pure domain types and invariants for ScreenSearch.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// Stable identifier for a captured frame.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct CaptureId(pub Uuid);

impl CaptureId {
    /// Creates a time-sortable identifier.
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for CaptureId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for CaptureId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Stable identifier for a durable analysis job.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct JobId(pub Uuid);

impl JobId {
    /// Creates a time-sortable identifier.
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for JobId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for JobId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Stable identifier for an indexed text chunk.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct ChunkId(pub Uuid);

impl ChunkId {
    /// Creates a time-sortable identifier.
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for ChunkId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ChunkId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// An unpersisted screen frame returned by a capture adapter.
#[derive(Clone, Debug, PartialEq)]
pub struct CapturedFrame {
    /// Capture time in UTC.
    pub captured_at: DateTime<Utc>,
    /// Logical monitor identifier supplied by the operating-system adapter.
    pub monitor_id: String,
    /// Foreground executable or application identifier.
    pub application: String,
    /// Foreground window title after privacy filtering.
    pub window_title: String,
    /// Width in physical pixels.
    pub width: u32,
    /// Height in physical pixels.
    pub height: u32,
    /// Encoded or raw bytes owned by the capture boundary.
    pub bytes: Vec<u8>,
    /// Media type describing the encoded bytes.
    pub media_type: String,
}

/// Immutable content-addressed asset metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AssetRef {
    /// BLAKE3 content hash.
    pub content_hash: String,
    /// Path relative to the configured asset root.
    pub relative_path: String,
    /// Media type of the stored payload.
    pub media_type: String,
    /// Stored payload size.
    pub byte_length: u64,
}

/// A capture ready for transactional persistence and job enqueueing.
#[derive(Clone, Debug, PartialEq)]
pub struct NewCapture {
    /// Assigned capture identifier.
    pub id: CaptureId,
    /// Capture time in UTC.
    pub captured_at: DateTime<Utc>,
    /// Logical monitor identifier.
    pub monitor_id: String,
    /// Foreground application.
    pub application: String,
    /// Foreground window title.
    pub window_title: String,
    /// Frame width.
    pub width: u32,
    /// Frame height.
    pub height: u32,
    /// Exact content fingerprint used for idempotency.
    pub fingerprint: String,
    /// Content-addressed asset metadata.
    pub asset: AssetRef,
}

/// Result of a transactional capture insertion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureDisposition {
    /// A new capture and analysis job were committed.
    Enqueued {
        /// Newly persisted capture.
        capture_id: CaptureId,
        /// Durable analysis job created in the same transaction.
        job_id: JobId,
    },
    /// An existing capture had the same fingerprint.
    Duplicate {
        /// Existing capture with the identical fingerprint.
        capture_id: CaptureId,
    },
    /// The frame was intentionally rejected before asset persistence.
    Skipped {
        /// Policy reason that prevented persistence.
        reason: CaptureSkipReason,
    },
}

/// Content-free reason that a capture attempt did not persist pixels.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureSkipReason {
    /// Capture was explicitly paused by the user.
    Paused,
    /// The durable analysis queue is above its configured high-water mark.
    Backpressured,
    /// The foreground application matched an exclusion rule.
    ExcludedApplication,
    /// The foreground window title matched an exclusion rule.
    ExcludedTitle,
    /// The frame was below the deterministic perceptual-change threshold.
    NearDuplicate,
}

impl std::fmt::Display for CaptureSkipReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Paused => "paused",
            Self::Backpressured => "backpressured",
            Self::ExcludedApplication => "excluded_application",
            Self::ExcludedTitle => "excluded_title",
            Self::NearDuplicate => "near_duplicate",
        };
        formatter.write_str(value)
    }
}

/// Content-free durable analysis queue measurements.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct QueueMetrics {
    /// Jobs waiting for a lease.
    pub pending: u64,
    /// Jobs currently holding a lease.
    pub running: u64,
    /// Total failed attempts recorded on live and dead jobs.
    pub retry_count: u64,
    /// Jobs that exhausted their retry budget.
    pub dead_letter_count: u64,
    /// Age of the oldest pending job, rounded down to seconds.
    pub oldest_pending_age_seconds: u64,
}

/// Versioned, user-controlled archive policy stored by the daemon.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArchiveSettings {
    /// Delete eligible captures older than this many days; `None` keeps captures by age.
    pub retention_days: Option<u32>,
    /// Maximum bytes used by immutable capture assets; `None` disables the asset budget.
    pub disk_budget_bytes: Option<u64>,
    /// Case-insensitive application substrings excluded before asset persistence.
    pub excluded_applications: Vec<String>,
    /// Case-insensitive window-title substrings excluded before asset persistence.
    pub excluded_titles: Vec<String>,
}

/// Source used to install or discover a local generation model.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSourceKind {
    /// Model was imported from a user-provided local file.
    Local,
    /// Model was downloaded explicitly from Hugging Face.
    HuggingFace,
    /// Model was discovered from packaged application resources.
    Bundled,
}

impl ModelSourceKind {
    /// Stable persistence value.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::HuggingFace => "hf",
            Self::Bundled => "bundled",
        }
    }

    /// Parses a persisted source value.
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        match value {
            "local" => Ok(Self::Local),
            "hf" => Ok(Self::HuggingFace),
            "bundled" => Ok(Self::Bundled),
            _ => Err(DomainError::InvalidModelCatalog(format!(
                "unknown model source kind {value}"
            ))),
        }
    }
}

/// Runtime metadata for one selectable GGUF generation model.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GenerationModel {
    /// Stable model identifier used by settings and diagnostics.
    pub id: String,
    /// Human-readable model name.
    pub display_name: String,
    /// Source used to acquire the model.
    pub source: ModelSourceKind,
    /// Hugging Face repository when `source` is `HuggingFace`.
    pub repository: Option<String>,
    /// File name inside the source repository or import directory.
    pub filename: String,
    /// Path relative to the generation-model root.
    pub relative_path: String,
    /// BLAKE3 hash of the GGUF file when known.
    pub content_hash: Option<String>,
    /// File size in bytes.
    pub byte_length: u64,
    /// Model architecture or family label.
    pub architecture: Option<String>,
    /// Quantization label such as `Q4_K_M`.
    pub quantization: Option<String>,
    /// Context window configured for evaluation.
    pub context_tokens: Option<u32>,
    /// Whether the model has a matching multimodal projector.
    pub supports_vision: bool,
    /// Whether this model is currently active for answer generation.
    pub active: bool,
}

impl GenerationModel {
    /// Validates bounded, content-free model metadata.
    pub fn validate(&self) -> Result<(), DomainError> {
        validate_model_text("model id", &self.id, 1, 128)?;
        validate_model_text("model display name", &self.display_name, 1, 160)?;
        validate_model_text("model filename", &self.filename, 1, 260)?;
        validate_model_text("model relative path", &self.relative_path, 1, 512)?;
        if self.relative_path.contains("..") || self.relative_path.starts_with('/') {
            return Err(DomainError::InvalidModelCatalog(
                "model relative path must stay below the model root".to_owned(),
            ));
        }
        if let Some(repository) = &self.repository {
            validate_model_text("model repository", repository, 1, 200)?;
        }
        if let Some(content_hash) = &self.content_hash {
            validate_model_text("model content hash", content_hash, 64, 64)?;
        }
        if self.byte_length == 0 {
            return Err(DomainError::InvalidModelCatalog(
                "model byte length must be non-zero".to_owned(),
            ));
        }
        Ok(())
    }
}

fn validate_model_text(
    label: &str,
    value: &str,
    minimum: usize,
    maximum: usize,
) -> Result<(), DomainError> {
    let length = value.trim().chars().count();
    if length < minimum || length > maximum {
        return Err(DomainError::InvalidModelCatalog(format!(
            "{label} must contain {minimum} to {maximum} characters"
        )));
    }
    Ok(())
}

impl ArchiveSettings {
    /// Validates bounded settings before they cross persistence or IPC boundaries.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self
            .retention_days
            .is_some_and(|days| days == 0 || days > 3_650)
        {
            return Err(DomainError::InvalidSettings(
                "retention days must be between 1 and 3650".to_owned(),
            ));
        }
        if self
            .disk_budget_bytes
            .is_some_and(|bytes| bytes < 256 * 1024 * 1024)
        {
            return Err(DomainError::InvalidSettings(
                "disk budget must be at least 256 MiB".to_owned(),
            ));
        }
        if self.excluded_applications.len() > 100 || self.excluded_titles.len() > 100 {
            return Err(DomainError::InvalidSettings(
                "at most 100 application and 100 title exclusions are allowed".to_owned(),
            ));
        }
        for pattern in self
            .excluded_applications
            .iter()
            .chain(&self.excluded_titles)
        {
            let length = pattern.trim().chars().count();
            if length == 0 || length > 128 {
                return Err(DomainError::InvalidSettings(
                    "exclusion patterns must contain 1 to 128 characters".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

/// Content-free archive storage measurements.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct StorageMetrics {
    /// Persisted captures.
    pub capture_count: u64,
    /// Immutable assets referenced by at least one capture.
    pub asset_count: u64,
    /// Encoded bytes occupied by referenced capture assets.
    pub asset_bytes: u64,
    /// OCR blocks available for evidence highlighting.
    pub ocr_block_count: u64,
    /// Search chunks indexed lexically and semantically.
    pub search_chunk_count: u64,
}

/// Explicit deletion selector accepted by the archive boundary.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DeleteCaptures {
    /// Specific capture identifiers to remove.
    pub capture_ids: Vec<CaptureId>,
    /// Remove captures strictly older than this timestamp.
    pub before: Option<DateTime<Utc>>,
    /// Remove every capture that is not currently leased for analysis.
    pub delete_all: bool,
}

/// Result of retention or explicit deletion.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeletionSummary {
    /// Capture rows removed transactionally with their derived data.
    pub captures_deleted: u64,
    /// Newly unreferenced assets placed on the durable cleanup queue.
    pub assets_scheduled: u64,
}

/// One durable, unreferenced asset cleanup task.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetCleanupTask {
    /// Content-addressed asset to remove idempotently.
    pub asset: AssetRef,
    /// Number of prior failed cleanup attempts.
    pub attempt: u32,
}

impl QueueMetrics {
    /// Work that is either waiting or currently being processed.
    pub fn depth(self) -> u64 {
        self.pending.saturating_add(self.running)
    }
}

/// A leased background analysis job.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnalysisJob {
    /// Job identifier.
    pub id: JobId,
    /// Capture to analyze.
    pub capture_id: CaptureId,
    /// Asset consumed by OCR and vision providers.
    pub asset: AssetRef,
    /// Zero-based retry count.
    pub attempt: u32,
}

/// A normalized rectangle relative to the full capture.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
    /// Left coordinate in the range 0 to 1.
    pub x: f32,
    /// Top coordinate in the range 0 to 1.
    pub y: f32,
    /// Width in the range 0 to 1.
    pub width: f32,
    /// Height in the range 0 to 1.
    pub height: f32,
}

impl BoundingBox {
    /// Validates that the rectangle is finite and remains inside the capture.
    pub fn validate(self) -> Result<Self, DomainError> {
        let values = [self.x, self.y, self.width, self.height];
        if values.iter().any(|value| !value.is_finite())
            || self.x < 0.0
            || self.y < 0.0
            || self.width < 0.0
            || self.height < 0.0
            || self.x + self.width > 1.0
            || self.y + self.height > 1.0
        {
            return Err(DomainError::InvalidBoundingBox);
        }
        Ok(self)
    }
}

/// One OCR result in reading order.
#[derive(Clone, Debug, PartialEq)]
pub struct OcrBlock {
    /// Zero-based reading order.
    pub reading_order: u32,
    /// Location within the capture.
    pub bounds: BoundingBox,
    /// Recognized text.
    pub text: String,
    /// Recognition confidence from 0 to 1 when exposed by the provider.
    pub confidence: Option<f32>,
    /// BCP-47 language tag when known.
    pub language: Option<String>,
}

/// Indexed text and its vector representation.
#[derive(Clone, Debug, PartialEq)]
pub struct IndexedChunk {
    /// Chunk identifier.
    pub id: ChunkId,
    /// Source capture.
    pub capture_id: CaptureId,
    /// Normalized searchable text.
    pub text: String,
    /// OCR block reading order used to recover positioned evidence.
    pub source_reading_order: u32,
    /// Embedding model revision.
    pub model_id: String,
    /// Fixed-dimension vector.
    pub embedding: Vec<f32>,
}

/// Retrieval paths contributing to a fused search hit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchMatchKind {
    /// Full-text retrieval only.
    Lexical,
    /// Vector retrieval only.
    Semantic,
    /// Both full-text and vector retrieval.
    Hybrid,
}

/// Atomic result written when an analysis job succeeds.
#[derive(Clone, Debug, PartialEq)]
pub struct AnalysisResult {
    /// Completed job.
    pub job_id: JobId,
    /// Source capture.
    pub capture_id: CaptureId,
    /// OCR blocks in reading order.
    pub blocks: Vec<OcrBlock>,
    /// Search chunks with embeddings.
    pub chunks: Vec<IndexedChunk>,
    /// OCR model revision.
    pub ocr_model_id: String,
}

/// One ranked hybrid retrieval result.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    /// Source chunk.
    pub chunk_id: ChunkId,
    /// Source capture.
    pub capture_id: CaptureId,
    /// Text passed to context assembly.
    pub text: String,
    /// Reciprocal-rank-fusion score.
    pub score: f64,
    /// Capture time in UTC.
    pub captured_at: DateTime<Utc>,
    /// Foreground application recorded with the capture.
    pub application: String,
    /// Privacy-filtered foreground window title.
    pub window_title: String,
    /// Capture width in pixels.
    pub width: u32,
    /// Capture height in pixels.
    pub height: u32,
    /// Immutable screenshot asset.
    pub asset: AssetRef,
    /// Positioned OCR regions supporting this hit.
    pub bounds: Vec<BoundingBox>,
    /// Retrieval paths contributing to the rank.
    pub match_kind: SearchMatchKind,
    /// OCR provider revision used for the evidence.
    pub ocr_model_id: String,
    /// Embedding provider revision used for semantic ranking.
    pub embedding_model_id: String,
}

/// Event emitted by citation-aware answer generation.
#[derive(Clone, Debug, PartialEq)]
pub enum SearchEvent {
    /// Retrieval evidence, emitted before answer tokens.
    Citation(Box<SearchHit>),
    /// One incremental text token or token group.
    Token(String),
    /// Terminal event containing the number of citations.
    Completed {
        /// Number of retrieval citations emitted for this answer.
        citation_count: usize,
        /// Content-free answer-generation terminal status.
        answer_status: String,
        /// Optional content-free explanation for the answer status.
        answer_message: Option<String>,
    },
}

/// Domain validation failures.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum DomainError {
    /// A bounding box was non-finite or outside normalized coordinates.
    #[error("bounding box must be finite and within normalized capture coordinates")]
    InvalidBoundingBox,
    /// A request that requires text received only whitespace.
    #[error("text must not be empty")]
    EmptyText,
    /// Archive settings exceeded a documented bound.
    #[error("invalid archive settings: {0}")]
    InvalidSettings(String),
    /// Model catalog metadata exceeded a documented bound.
    #[error("invalid model catalog: {0}")]
    InvalidModelCatalog(String),
}

#[cfg(test)]
mod tests {
    use super::{ArchiveSettings, BoundingBox};

    #[test]
    fn bounding_box_rejects_coordinates_outside_capture() {
        let bounds = BoundingBox {
            x: 0.8,
            y: 0.0,
            width: 0.3,
            height: 1.0,
        };

        assert!(bounds.validate().is_err());
    }

    #[test]
    fn archive_settings_reject_unbounded_or_empty_values() {
        assert!(
            ArchiveSettings {
                retention_days: Some(0),
                ..ArchiveSettings::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            ArchiveSettings {
                excluded_titles: vec![String::new()],
                ..ArchiveSettings::default()
            }
            .validate()
            .is_err()
        );
    }
}
