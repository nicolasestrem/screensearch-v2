//! Explicitly ignored scale harness for the ten-million-capture operating target.

use std::time::Instant;

use chrono::Utc;
use screensearch_domain::{
    AnalysisResult, AssetRef, BoundingBox, CaptureDisposition, CaptureId, ChunkId, IndexedChunk,
    NewCapture, OcrBlock,
};
use screensearch_model_runtime::FakeEmbeddingEngine;
use screensearch_persistence::LibSqlArchive;
use screensearch_ports::{ArchiveRepository, EmbeddingEngine};
use tempfile::TempDir;

#[tokio::test]
#[ignore = "long-running benchmark; set SCREENSEARCH_RUN_SCALE_BENCH=1 and run explicitly"]
async fn synthetic_ten_million_capture_index() {
    assert_eq!(
        std::env::var("SCREENSEARCH_RUN_SCALE_BENCH").as_deref(),
        Ok("1"),
        "explicit opt-in is required"
    );
    let target = std::env::var("SCREENSEARCH_SCALE_ROWS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(10_000_000);
    let directory = TempDir::new().unwrap();
    let repository = LibSqlArchive::open(directory.path().join("scale.db"))
        .await
        .unwrap();
    repository.migrate().await.unwrap();
    let embeddings = FakeEmbeddingEngine;
    let vector = embeddings
        .embed("synthetic searchable screen")
        .await
        .unwrap();
    let started = Instant::now();

    for index in 0..target {
        let capture_id = CaptureId::new();
        let disposition = repository
            .enqueue_capture(NewCapture {
                id: capture_id,
                captured_at: Utc::now(),
                monitor_id: "scale-monitor".to_owned(),
                application: "scale.exe".to_owned(),
                window_title: format!("Synthetic screen {index}"),
                width: 1920,
                height: 1080,
                fingerprint: format!("synthetic-{index:016x}"),
                asset: AssetRef {
                    content_hash: "synthetic-asset".to_owned(),
                    relative_path: "sy/synthetic-asset.blob".to_owned(),
                    media_type: "application/octet-stream".to_owned(),
                    byte_length: 1,
                },
            })
            .await
            .unwrap();
        let CaptureDisposition::Enqueued { job_id, .. } = disposition else {
            panic!("synthetic captures must be unique");
        };
        let job = repository.claim_job("scale-worker").await.unwrap().unwrap();
        assert_eq!(job.id, job_id);
        let text = format!("synthetic searchable screen number {index}");
        repository
            .complete_analysis(AnalysisResult {
                job_id,
                capture_id,
                blocks: vec![OcrBlock {
                    reading_order: 0,
                    bounds: BoundingBox {
                        x: 0.0,
                        y: 0.0,
                        width: 1.0,
                        height: 0.1,
                    },
                    text: text.clone(),
                    confidence: Some(1.0),
                    language: Some("en".to_owned()),
                }],
                chunks: vec![IndexedChunk {
                    id: ChunkId::new(),
                    capture_id,
                    text,
                    source_reading_order: 0,
                    model_id: embeddings.model_id().to_owned(),
                    embedding: vector.clone(),
                }],
                ocr_model_id: "scale-ocr-v1".to_owned(),
            })
            .await
            .unwrap();

        if (index + 1) % 100_000 == 0 {
            eprintln!("indexed {} captures in {:?}", index + 1, started.elapsed());
        }
    }

    let search_started = Instant::now();
    let hits = repository
        .hybrid_search(
            "synthetic searchable screen",
            &vector,
            embeddings.model_id(),
            10,
        )
        .await
        .unwrap();
    eprintln!(
        "indexed {target} captures in {:?}; hybrid search took {:?}",
        started.elapsed(),
        search_started.elapsed()
    );
    assert!(!hits.is_empty());
}
