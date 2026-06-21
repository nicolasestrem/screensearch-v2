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
}

#[cfg(test)]
mod tests {
    use super::BoundingBox;

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
}
