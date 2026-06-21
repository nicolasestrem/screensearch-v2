//! Manual evidence-loop verification against a local Windows smoke archive.

use std::path::PathBuf;

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
    let vector = embeddings.embed(&query).await.unwrap();
    let hits = repository
        .hybrid_search(&query, &vector, embeddings.model_id(), 8)
        .await
        .unwrap();

    assert!(!hits.is_empty(), "the smoke query returned no evidence");
    let assets = FileAssetStore::new(root.join("assets"));
    for hit in hits {
        assert!(assets.resolve(&hit.asset).unwrap().exists());
        assert!(!hit.bounds.is_empty());
        assert!(!hit.application.is_empty());
    }
}
