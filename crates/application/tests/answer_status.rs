//! Asserts every terminal `answer_status` branch of `SearchService` with test doubles.

use std::sync::Arc;

use async_trait::async_trait;
use futures::{StreamExt, stream};
use screensearch_application::SearchService;
use screensearch_domain::{
    AnalysisJob, AnalysisResult, ArchiveSettings, AssetCleanupTask, AssetRef, CaptureDisposition,
    CaptureId, DeleteCaptures, DeletionSummary, GenerationModel, NewCapture, QueueMetrics,
    SearchEvent, SearchHit, SearchMatchKind, StorageMetrics,
};
use screensearch_ports::{
    ArchiveRepository, EmbeddingEngine, PortError, TextGenerator, TokenStream,
};

/// Archive double whose only meaningful behavior is the configured hybrid-search result.
struct FakeRepository {
    hits: Vec<SearchHit>,
}

#[async_trait]
impl ArchiveRepository for FakeRepository {
    async fn hybrid_search(
        &self,
        _query: &str,
        _embedding: &[f32],
        _model_id: &str,
        _limit: usize,
    ) -> Result<Vec<SearchHit>, PortError> {
        Ok(self.hits.clone())
    }

    async fn migrate(&self) -> Result<(), PortError> {
        unimplemented!("not exercised by answer-status tests")
    }
    async fn enqueue_capture(&self, _capture: NewCapture) -> Result<CaptureDisposition, PortError> {
        unimplemented!("not exercised by answer-status tests")
    }
    async fn claim_job(&self, _worker_id: &str) -> Result<Option<AnalysisJob>, PortError> {
        unimplemented!("not exercised by answer-status tests")
    }
    async fn complete_analysis(&self, _result: AnalysisResult) -> Result<(), PortError> {
        unimplemented!("not exercised by answer-status tests")
    }
    async fn fail_job(&self, _job: &AnalysisJob, _reason: &str) -> Result<(), PortError> {
        unimplemented!("not exercised by answer-status tests")
    }
    async fn capture_asset(&self, _capture_id: CaptureId) -> Result<Option<AssetRef>, PortError> {
        unimplemented!("not exercised by answer-status tests")
    }
    async fn queue_metrics(&self) -> Result<QueueMetrics, PortError> {
        unimplemented!("not exercised by answer-status tests")
    }
    async fn archive_settings(&self) -> Result<ArchiveSettings, PortError> {
        unimplemented!("not exercised by answer-status tests")
    }
    async fn update_archive_settings(&self, _settings: ArchiveSettings) -> Result<(), PortError> {
        unimplemented!("not exercised by answer-status tests")
    }
    async fn storage_metrics(&self) -> Result<StorageMetrics, PortError> {
        unimplemented!("not exercised by answer-status tests")
    }
    async fn apply_retention(
        &self,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<DeletionSummary, PortError> {
        unimplemented!("not exercised by answer-status tests")
    }
    async fn delete_captures(
        &self,
        _request: DeleteCaptures,
    ) -> Result<DeletionSummary, PortError> {
        unimplemented!("not exercised by answer-status tests")
    }
    async fn claim_asset_cleanup(&self) -> Result<Option<AssetCleanupTask>, PortError> {
        unimplemented!("not exercised by answer-status tests")
    }
    async fn complete_asset_cleanup(&self, _content_hash: &str) -> Result<(), PortError> {
        unimplemented!("not exercised by answer-status tests")
    }
    async fn fail_asset_cleanup(
        &self,
        _content_hash: &str,
        _reason: &str,
    ) -> Result<(), PortError> {
        unimplemented!("not exercised by answer-status tests")
    }
    async fn generation_models(&self) -> Result<Vec<GenerationModel>, PortError> {
        unimplemented!("not exercised by answer-status tests")
    }
    async fn upsert_generation_model(&self, _model: GenerationModel) -> Result<(), PortError> {
        unimplemented!("not exercised by answer-status tests")
    }
    async fn select_generation_model(&self, _model_id: &str) -> Result<(), PortError> {
        unimplemented!("not exercised by answer-status tests")
    }
    async fn clear_active_generation_model(&self) -> Result<(), PortError> {
        unimplemented!("not exercised by answer-status tests")
    }
    async fn active_generation_model(&self) -> Result<Option<GenerationModel>, PortError> {
        unimplemented!("not exercised by answer-status tests")
    }
    async fn delete_generation_model(
        &self,
        _model_id: &str,
    ) -> Result<Option<GenerationModel>, PortError> {
        unimplemented!("not exercised by answer-status tests")
    }
}

struct FixedEmbeddingEngine;

#[async_trait]
impl EmbeddingEngine for FixedEmbeddingEngine {
    fn model_id(&self) -> &'static str {
        "test-embedding-384"
    }
    fn dimensions(&self) -> usize {
        384
    }
    async fn embed(&self, _text: &str) -> Result<Vec<f32>, PortError> {
        Ok(vec![0.0; 384])
    }
}

struct AnsweringGenerator;

#[async_trait]
impl TextGenerator for AnsweringGenerator {
    async fn generate(&self, _prompt: String) -> Result<TokenStream, PortError> {
        Ok(Box::pin(stream::iter(vec![
            Ok::<_, PortError>("The ".to_owned()),
            Ok("answer.".to_owned()),
        ])))
    }
}

struct StreamFailingGenerator;

#[async_trait]
impl TextGenerator for StreamFailingGenerator {
    async fn generate(&self, _prompt: String) -> Result<TokenStream, PortError> {
        Ok(Box::pin(stream::iter(vec![
            Ok("partial".to_owned()),
            Err(PortError::Internal("token decode failed".to_owned())),
        ])))
    }
}

struct FailingGenerator {
    error: PortError,
}

#[async_trait]
impl TextGenerator for FailingGenerator {
    async fn generate(&self, _prompt: String) -> Result<TokenStream, PortError> {
        Err(self.error.clone())
    }
}

struct UnreachableGenerator;

#[async_trait]
impl TextGenerator for UnreachableGenerator {
    async fn generate(&self, _prompt: String) -> Result<TokenStream, PortError> {
        unreachable!("generation must be skipped when there is no evidence")
    }
}

fn sample_hit() -> SearchHit {
    SearchHit {
        chunk_id: screensearch_domain::ChunkId::new(),
        capture_id: CaptureId::new(),
        text: "Quarterly revenue summary".to_owned(),
        score: 1.0,
        captured_at: chrono::Utc::now(),
        application: "notes.exe".to_owned(),
        window_title: "Notes".to_owned(),
        width: 1920,
        height: 1080,
        asset: AssetRef {
            content_hash: "a".repeat(64),
            relative_path: "aa/model.png".to_owned(),
            media_type: "image/png".to_owned(),
            byte_length: 1,
        },
        bounds: Vec::new(),
        match_kind: SearchMatchKind::Lexical,
        ocr_model_id: "test-ocr".to_owned(),
        embedding_model_id: "test-embedding-384".to_owned(),
    }
}

fn service(hits: Vec<SearchHit>, generator: Arc<dyn TextGenerator>) -> SearchService {
    SearchService::new(
        Arc::new(FakeRepository { hits }),
        Arc::new(FixedEmbeddingEngine),
        generator,
    )
}

async fn terminal_status(service: &SearchService, generate_answer: bool) -> (usize, String) {
    let mut stream = service
        .search_with_options("what was visible?", 10, generate_answer)
        .await
        .expect("search starts");
    let mut citations = 0_usize;
    let mut completed = None;
    while let Some(event) = stream.next().await {
        match event.expect("event is ok") {
            SearchEvent::Citation(_) => citations += 1,
            SearchEvent::Token(_) => {}
            SearchEvent::Completed {
                citation_count,
                answer_status,
                ..
            } => {
                assert_eq!(citation_count, citations);
                completed = Some((citation_count, answer_status));
            }
        }
    }
    completed.expect("stream yields a terminal event")
}

#[tokio::test]
async fn evidence_only_when_generation_is_disabled() {
    let service = service(vec![sample_hit()], Arc::new(UnreachableGenerator));
    let (citations, status) = terminal_status(&service, false).await;
    assert_eq!(citations, 1);
    assert_eq!(status, "evidence_only");
}

#[tokio::test]
async fn no_evidence_skips_the_generator() {
    let service = service(Vec::new(), Arc::new(UnreachableGenerator));
    let (citations, status) = terminal_status(&service, true).await;
    assert_eq!(citations, 0);
    assert_eq!(status, "no_evidence");
}

#[tokio::test]
async fn answered_when_the_generator_streams_to_completion() {
    let service = service(vec![sample_hit()], Arc::new(AnsweringGenerator));
    let (_, status) = terminal_status(&service, true).await;
    assert_eq!(status, "answered");
}

#[tokio::test]
async fn generation_failed_when_a_token_errors_mid_stream() {
    let service = service(vec![sample_hit()], Arc::new(StreamFailingGenerator));
    let (_, status) = terminal_status(&service, true).await;
    assert_eq!(status, "generation_failed");
}

#[tokio::test]
async fn model_missing_when_the_generator_is_unavailable() {
    let service = service(
        vec![sample_hit()],
        Arc::new(FailingGenerator {
            error: PortError::Unavailable("no model installed".to_owned()),
        }),
    );
    let (_, status) = terminal_status(&service, true).await;
    assert_eq!(status, "model_missing");
}

#[tokio::test]
async fn cancelled_when_the_generator_reports_a_transient_failure() {
    let service = service(
        vec![sample_hit()],
        Arc::new(FailingGenerator {
            error: PortError::Transient("worker restarting".to_owned()),
        }),
    );
    let (_, status) = terminal_status(&service, true).await;
    assert_eq!(status, "cancelled");
}

#[tokio::test]
async fn generation_failed_for_other_upfront_errors() {
    let service = service(
        vec![sample_hit()],
        Arc::new(FailingGenerator {
            error: PortError::Internal("boom".to_owned()),
        }),
    );
    let (_, status) = terminal_status(&service, true).await;
    assert_eq!(status, "generation_failed");
}
