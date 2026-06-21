//! Use-case orchestration for capture ingestion, durable analysis, and cited search.

use std::{fmt::Write as _, pin::Pin, sync::Arc};

use futures::{Stream, StreamExt, stream};
use screensearch_domain::{
    AnalysisResult, CaptureDisposition, CaptureId, ChunkId, IndexedChunk, NewCapture, SearchEvent,
};
use screensearch_ports::{
    ArchiveRepository, AssetStore, CaptureSource, EmbeddingEngine, OcrEngine, PortError,
    TextGenerator,
};
use tracing::{info, warn};

/// Stream of retrieval citations, answer tokens, and a terminal event.
pub type SearchEventStream = Pin<Box<dyn Stream<Item = Result<SearchEvent, PortError>> + Send>>;

/// Coordinates one capture without owning any adapter implementation.
pub struct IngestService {
    source: Arc<dyn CaptureSource>,
    assets: Arc<dyn AssetStore>,
    repository: Arc<dyn ArchiveRepository>,
}

impl IngestService {
    /// Creates an ingest service from inward-facing ports.
    pub fn new(
        source: Arc<dyn CaptureSource>,
        assets: Arc<dyn AssetStore>,
        repository: Arc<dyn ArchiveRepository>,
    ) -> Self {
        Self {
            source,
            assets,
            repository,
        }
    }

    /// Captures, fingerprints, stores, and transactionally enqueues one frame.
    pub async fn capture_once(&self) -> Result<CaptureDisposition, PortError> {
        let frame = self.source.capture().await?;
        let fingerprint = blake3::hash(&frame.bytes).to_hex().to_string();
        let asset = self.assets.put(&frame.bytes, &frame.media_type).await?;
        let capture = NewCapture {
            id: CaptureId::new(),
            captured_at: frame.captured_at,
            monitor_id: frame.monitor_id,
            application: frame.application,
            window_title: frame.window_title,
            width: frame.width,
            height: frame.height,
            fingerprint,
            asset,
        };

        let disposition = self.repository.enqueue_capture(capture).await?;
        info!(?disposition, "capture persisted");
        Ok(disposition)
    }
}

/// Processes durable OCR and embedding jobs with idempotent repository commits.
pub struct AnalysisService {
    repository: Arc<dyn ArchiveRepository>,
    ocr: Arc<dyn OcrEngine>,
    embeddings: Arc<dyn EmbeddingEngine>,
    worker_id: String,
}

impl AnalysisService {
    /// Creates a background analysis service.
    pub fn new(
        repository: Arc<dyn ArchiveRepository>,
        ocr: Arc<dyn OcrEngine>,
        embeddings: Arc<dyn EmbeddingEngine>,
        worker_id: impl Into<String>,
    ) -> Self {
        Self {
            repository,
            ocr,
            embeddings,
            worker_id: worker_id.into(),
        }
    }

    /// Claims and processes at most one durable job.
    pub async fn process_one(&self) -> Result<bool, PortError> {
        let Some(job) = self.repository.claim_job(&self.worker_id).await? else {
            return Ok(false);
        };

        let operation = async {
            let blocks = self.ocr.recognize(&job.asset).await?;
            let mut chunks = Vec::new();
            for block in &blocks {
                let text = block.text.trim();
                if text.is_empty() {
                    continue;
                }
                let embedding = self.embeddings.embed(text).await?;
                if embedding.len() != self.embeddings.dimensions() {
                    return Err(PortError::InvalidData(format!(
                        "embedding provider returned {} dimensions, expected {}",
                        embedding.len(),
                        self.embeddings.dimensions()
                    )));
                }
                chunks.push(IndexedChunk {
                    id: ChunkId::new(),
                    capture_id: job.capture_id,
                    text: text.to_owned(),
                    source_reading_order: block.reading_order,
                    model_id: self.embeddings.model_id().to_owned(),
                    embedding,
                });
            }

            self.repository
                .complete_analysis(AnalysisResult {
                    job_id: job.id,
                    capture_id: job.capture_id,
                    blocks,
                    chunks,
                    ocr_model_id: self.ocr.model_id().to_owned(),
                })
                .await
        }
        .await;

        if let Err(error) = operation {
            warn!(job_id = %job.id, %error, "analysis job failed");
            self.repository.fail_job(&job, &error.to_string()).await?;
            return Err(error);
        }

        Ok(true)
    }
}

/// Runs hybrid retrieval and streams a citation-backed local answer.
pub struct SearchService {
    repository: Arc<dyn ArchiveRepository>,
    embeddings: Arc<dyn EmbeddingEngine>,
    generator: Arc<dyn TextGenerator>,
}

impl SearchService {
    /// Creates a search service.
    pub fn new(
        repository: Arc<dyn ArchiveRepository>,
        embeddings: Arc<dyn EmbeddingEngine>,
        generator: Arc<dyn TextGenerator>,
    ) -> Self {
        Self {
            repository,
            embeddings,
            generator,
        }
    }

    /// Retrieves evidence and returns citations followed by streamed answer tokens.
    pub async fn search(&self, query: &str, limit: usize) -> Result<SearchEventStream, PortError> {
        self.search_with_options(query, limit, true).await
    }

    /// Retrieves evidence and optionally invokes the local text generator.
    pub async fn search_with_options(
        &self,
        query: &str,
        limit: usize,
        generate_answer: bool,
    ) -> Result<SearchEventStream, PortError> {
        let query = query.trim();
        if query.is_empty() {
            return Err(PortError::InvalidData("search query is empty".to_owned()));
        }

        let embedding = self.embeddings.embed(query).await?;
        let hits = self
            .repository
            .hybrid_search(
                query,
                &embedding,
                self.embeddings.model_id(),
                limit.clamp(1, 50),
            )
            .await?;
        let citation_count = hits.len();
        let prompt = assemble_prompt(query, &hits);
        let tokens = if generate_answer && citation_count > 0 {
            self.generator.generate(prompt).await?
        } else {
            Box::pin(stream::empty())
        };

        let citations = stream::iter(
            hits.into_iter()
                .map(|hit| Ok(SearchEvent::Citation(Box::new(hit)))),
        );
        let tokens = tokens.map(|result| result.map(SearchEvent::Token));
        let completed = stream::once(async move { Ok(SearchEvent::Completed { citation_count }) });

        Ok(Box::pin(citations.chain(tokens).chain(completed)))
    }
}

fn assemble_prompt(query: &str, hits: &[screensearch_domain::SearchHit]) -> String {
    let mut prompt = String::from(
        "Answer only from the supplied local captures. Cite capture identifiers in brackets.\n\n",
    );
    for hit in hits {
        let _ = writeln!(&mut prompt, "[{}] {}", hit.capture_id, hit.text);
    }
    prompt.push_str("\nQuestion: ");
    prompt.push_str(query);
    prompt
}

#[cfg(test)]
mod tests {
    use screensearch_domain::{CaptureId, ChunkId, SearchHit};

    use super::assemble_prompt;

    #[test]
    fn prompt_preserves_capture_citations() {
        let capture_id = CaptureId::new();
        let prompt = assemble_prompt(
            "what was visible?",
            &[SearchHit {
                chunk_id: ChunkId::new(),
                capture_id,
                text: "Quarterly plan".to_owned(),
                score: 1.0,
                captured_at: chrono::Utc::now(),
                application: "test.exe".to_owned(),
                window_title: "Test".to_owned(),
                width: 1,
                height: 1,
                asset: screensearch_domain::AssetRef {
                    content_hash: "hash".to_owned(),
                    relative_path: "aa/hash.png".to_owned(),
                    media_type: "image/png".to_owned(),
                    byte_length: 1,
                },
                bounds: Vec::new(),
                match_kind: screensearch_domain::SearchMatchKind::Lexical,
                ocr_model_id: "test-ocr".to_owned(),
                embedding_model_id: "test-embedding".to_owned(),
            }],
        );

        assert!(prompt.contains(&format!("[{capture_id}] Quarterly plan")));
    }
}
