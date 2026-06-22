//! Manual evidence-loop verification against a local Windows smoke archive.

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use screensearch_application::plan_search;
use screensearch_domain::SearchFilters;
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
            .hybrid_search(
                &query,
                &vector,
                embeddings.model_id(),
                &SearchFilters::default(),
                8,
            )
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

#[tokio::test]
#[ignore = "requires a populated SCREENSEARCH_DATA_DIR and local model cache"]
async fn local_archive_answer_smoke_is_content_free() {
    let root = PathBuf::from(
        std::env::var_os("SCREENSEARCH_DATA_DIR")
            .expect("SCREENSEARCH_DATA_DIR must point to a smoke archive"),
    );
    let repository = LibSqlArchive::open(root.join("screensearch.db"))
        .await
        .unwrap();
    repository.migrate().await.unwrap();
    let embeddings = FastEmbedEngine::new(root.join("models"));
    let prompts = [
        (
            "telegram_noon",
            "What did I talk about on Telegram around Noon",
        ),
        (
            "github_largest_pr_today",
            "What has been my largest PR on GitHub today?",
        ),
        (
            "codex_settings_early_afternoon",
            "What did I see in Codex settings early afternoon?",
        ),
        (
            "amazon_book_afternoon",
            "Which book did I check on Amazon this afternoon?",
        ),
    ];
    let now = chrono::Local::now().fixed_offset();
    for (label, prompt) in prompts {
        let plan = plan_search(prompt, now).unwrap();
        let embedding_query;
        let embedding_text = if plan.retrieval_query.is_empty() {
            if plan.filters.source_terms.is_empty() {
                plan.original_query.as_str()
            } else {
                embedding_query = plan.filters.source_terms.join(" ");
                embedding_query.as_str()
            }
        } else {
            plan.retrieval_query.as_str()
        };
        let vector = embeddings.embed(embedding_text).await.unwrap();
        let started = Instant::now();
        let hits = repository
            .hybrid_search(
                &plan.retrieval_query,
                &vector,
                embeddings.model_id(),
                &plan.filters,
                20,
            )
            .await
            .unwrap();
        let latency_ms = started.elapsed().as_millis();
        let citation_count = hits.len();
        let status = if citation_count == 0 {
            "no_evidence"
        } else {
            "evidence_only"
        };
        eprintln!(
            "answer_smoke label={label} retrieval_query={:?} source_terms={} captured_after={} captured_before={} citation_count={citation_count} answer_status={status} latency_ms={latency_ms} required_citations_present={}",
            plan.retrieval_query,
            plan.filters.source_terms.join(","),
            plan.filters
                .captured_after
                .map(|value| value.to_rfc3339())
                .unwrap_or_default(),
            plan.filters
                .captured_before
                .map(|value| value.to_rfc3339())
                .unwrap_or_default(),
            citation_count > 0,
        );
    }
}

fn percentile(samples: &[Duration], percentile: usize) -> Duration {
    let index = (samples.len().saturating_sub(1) * percentile) / 100;
    samples[index]
}
