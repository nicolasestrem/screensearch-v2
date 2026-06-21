//! Manual evidence-loop verification against a local Windows smoke archive.

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use screensearch_model_runtime::FastEmbedEngine;
use screensearch_persistence::{FileAssetStore, LibSqlArchive};
use screensearch_ports::{ArchiveRepository, EmbeddingEngine};

#[tokio::test]
#[ignore = "requires a populated SCREENSEARCH_DATA_DIR and local model cache"]
async fn real_archive_returns_resolvable_visual_evidence() {
    let root = PathBuf::from(
        std::env::var_os("SCREENSEARCH_DATA_DIR")
            .expect("SCREENSEARCH_DATA_DIR must point to a smoke archive"),
    );
    let query = std::env::var("SCREENSEARCH_SMOKE_QUERY").unwrap_or_else(|_| "project".to_owned());
    let repository = LibSqlArchive::open(root.join("screensearch.db"))
        .await
        .unwrap();
    repository.migrate().await.unwrap();
    let embeddings = FastEmbedEngine::new(root.join("models"));
    let embedding_started = Instant::now();
    let vector = embeddings.embed(&query).await.unwrap();
    let embedding_elapsed = embedding_started.elapsed();
    let mut search_durations = Vec::with_capacity(20);
    let mut hits = Vec::new();
    for _ in 0..20 {
        let search_started = Instant::now();
        hits = repository
            .hybrid_search(&query, &vector, embeddings.model_id(), 8)
            .await
            .unwrap();
        search_durations.push(search_started.elapsed());
    }
    search_durations.sort_unstable();
    let search_p50 = percentile(&search_durations, 50);
    let search_p95 = percentile(&search_durations, 95);
    let search_p99 = percentile(&search_durations, 99);
    eprintln!(
        "query_embedding={embedding_elapsed:?} hybrid_p50={search_p50:?} hybrid_p95={search_p95:?} hybrid_p99={search_p99:?} hits={}",
        hits.len()
    );
    assert!(search_p95 < Duration::from_secs(1));

    assert!(!hits.is_empty(), "the smoke query returned no evidence");
    let assets = FileAssetStore::new(root.join("assets"));
    for hit in hits {
        assert!(assets.resolve(&hit.asset).unwrap().exists());
        assert!(!hit.bounds.is_empty());
        assert!(!hit.application.is_empty());
    }
}

fn percentile(samples: &[Duration], percentile: usize) -> Duration {
    let index = (samples.len().saturating_sub(1) * percentile) / 100;
    samples[index]
}
