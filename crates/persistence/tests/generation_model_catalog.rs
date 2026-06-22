//! Deterministic catalog invariants for the generation-model selection feature.

use screensearch_domain::{GenerationModel, ModelSourceKind};
use screensearch_persistence::LibSqlArchive;
use screensearch_ports::{ArchiveRepository, PortError};

fn model(id: &str, display_name: &str, active: bool) -> GenerationModel {
    GenerationModel {
        id: id.to_owned(),
        display_name: display_name.to_owned(),
        source: ModelSourceKind::Local,
        repository: None,
        filename: format!("{id}.gguf"),
        relative_path: format!("{id}/{id}.gguf"),
        content_hash: Some("a".repeat(64)),
        byte_length: 2_048,
        architecture: Some("Qwen".to_owned()),
        quantization: Some("Q4_K_M".to_owned()),
        context_tokens: Some(2_048),
        supports_vision: false,
        active,
    }
}

async fn archive() -> LibSqlArchive {
    let repository = LibSqlArchive::in_memory().await.unwrap();
    repository.migrate().await.unwrap();
    repository
}

#[tokio::test]
async fn upsert_lists_active_first_then_by_display_name() {
    let repository = archive().await;
    repository
        .upsert_generation_model(model("zeta", "Zeta", false))
        .await
        .unwrap();
    repository
        .upsert_generation_model(model("alpha", "Alpha", false))
        .await
        .unwrap();
    repository
        .upsert_generation_model(model("midnight", "Midnight", true))
        .await
        .unwrap();

    let models = repository.generation_models().await.unwrap();
    let ids: Vec<&str> = models.iter().map(|model| model.id.as_str()).collect();

    assert_eq!(ids, ["midnight", "alpha", "zeta"]);
    assert!(models[0].active);
}

#[tokio::test]
async fn selecting_a_model_keeps_exactly_one_active() {
    let repository = archive().await;
    repository
        .upsert_generation_model(model("alpha", "Alpha", false))
        .await
        .unwrap();
    repository
        .upsert_generation_model(model("beta", "Beta", false))
        .await
        .unwrap();

    repository.select_generation_model("alpha").await.unwrap();
    assert_eq!(
        repository
            .active_generation_model()
            .await
            .unwrap()
            .map(|model| model.id),
        Some("alpha".to_owned())
    );

    repository.select_generation_model("beta").await.unwrap();
    let active: Vec<String> = repository
        .generation_models()
        .await
        .unwrap()
        .into_iter()
        .filter(|model| model.active)
        .map(|model| model.id)
        .collect();

    assert_eq!(active, ["beta"]);
}

#[tokio::test]
async fn upserting_an_active_model_deactivates_the_previous_one() {
    let repository = archive().await;
    repository
        .upsert_generation_model(model("alpha", "Alpha", true))
        .await
        .unwrap();
    repository
        .upsert_generation_model(model("beta", "Beta", true))
        .await
        .unwrap();

    let active: Vec<String> = repository
        .generation_models()
        .await
        .unwrap()
        .into_iter()
        .filter(|model| model.active)
        .map(|model| model.id)
        .collect();

    assert_eq!(active, ["beta"]);
}

#[tokio::test]
async fn deleting_the_active_model_is_denied() {
    let repository = archive().await;
    repository
        .upsert_generation_model(model("alpha", "Alpha", true))
        .await
        .unwrap();

    let error = repository
        .delete_generation_model("alpha")
        .await
        .unwrap_err();
    assert!(matches!(error, PortError::Denied(_)));
    assert_eq!(repository.generation_models().await.unwrap().len(), 1);
}

#[tokio::test]
async fn deleting_an_inactive_model_removes_it() {
    let repository = archive().await;
    repository
        .upsert_generation_model(model("alpha", "Alpha", false))
        .await
        .unwrap();

    let deleted = repository
        .delete_generation_model("alpha")
        .await
        .unwrap()
        .map(|model| model.id);

    assert_eq!(deleted, Some("alpha".to_owned()));
    assert!(repository.generation_models().await.unwrap().is_empty());
}

#[tokio::test]
async fn selecting_an_unregistered_model_is_rejected() {
    let repository = archive().await;

    let error = repository
        .select_generation_model("ghost")
        .await
        .unwrap_err();
    assert!(matches!(error, PortError::InvalidData(_)));
}

#[tokio::test]
async fn clearing_active_keeps_catalog_rows() {
    let repository = archive().await;
    repository
        .upsert_generation_model(model("alpha", "Alpha", true))
        .await
        .unwrap();

    repository.clear_active_generation_model().await.unwrap();

    assert!(
        repository
            .active_generation_model()
            .await
            .unwrap()
            .is_none()
    );
    let models = repository.generation_models().await.unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "alpha");
    assert!(!models[0].active);
}
