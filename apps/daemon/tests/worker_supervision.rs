//! Gated Windows integration tests for model-worker supervision primitives.
//!
//! These spawn the real `screensearch-model-worker` binary and exercise the runtime
//! pieces the daemon's supervisor relies on: readiness, the parent lifeline (orphan
//! prevention), kill-then-restart recovery, non-blocking health during/after a
//! cancelled generation, and the raw idle-unload primitive. The `RestartPolicy`
//! decision logic and the catalog's "clear-active keeps rows" invariant are covered by
//! fast unit tests; this file proves the live process plumbing those rely on.
//!
//! All cases are `#[ignore]`d and require `SCREENSEARCH_RUN_WORKER_IT=1`. Build the
//! worker first: `cargo build -p screensearch-model-worker`. Generation cases also need
//! `SCREENSEARCH_TEST_GGUF` to point at a local `.gguf`; they skip cleanly if it is unset.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use screensearch_ipc::{
    IpcError,
    transport::{IpcClient, WorkerLifeline, create_worker_lifeline},
    v1::{
        RequestEnvelope, WorkerGenerationRequest, WorkerHealthRequest, WorkerHealthResponse,
        WorkerUnloadRequest, request_envelope, response_envelope, worker_generation_event,
    },
};
use tempfile::TempDir;
use tokio::process::Child;
use uuid::Uuid;

const GENERATION_PROMPT: &str = "Answer only from the supplied local captures. Cite capture identifiers in brackets.\n\n[00000000-0000-7000-8000-000000000001] The ScreenSearch benchmark phrase is cobalt window.\n\nQuestion: What benchmark phrase was visible?";

fn require_opt_in() {
    assert_eq!(
        std::env::var("SCREENSEARCH_RUN_WORKER_IT").as_deref(),
        Ok("1"),
        "explicit opt-in is required; set SCREENSEARCH_RUN_WORKER_IT=1"
    );
}

fn test_gguf() -> Option<PathBuf> {
    std::env::var_os("SCREENSEARCH_TEST_GGUF")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

fn worker_binary() -> PathBuf {
    let name = if cfg!(windows) {
        "screensearch-model-worker.exe"
    } else {
        "screensearch-model-worker"
    };
    for ancestor in Path::new(env!("CARGO_MANIFEST_DIR")).ancestors() {
        let candidate = ancestor.join("target").join("debug").join(name);
        if candidate.is_file() {
            return candidate;
        }
    }
    panic!("model worker binary not found; run `cargo build -p screensearch-model-worker` first");
}

struct TestEnv {
    _directory: TempDir,
    binary: PathBuf,
    asset_root: PathBuf,
    model_root: PathBuf,
    pipe: String,
}

impl TestEnv {
    fn new() -> Self {
        let directory = TempDir::new().unwrap();
        let asset_root = directory.path().join("assets");
        let model_root = directory.path().join("models");
        std::fs::create_dir_all(&asset_root).unwrap();
        std::fs::create_dir_all(&model_root).unwrap();
        Self {
            _directory: directory,
            binary: worker_binary(),
            asset_root,
            model_root,
            pipe: unique_pipe("worker"),
        }
    }
}

fn unique_pipe(role: &str) -> String {
    format!(
        r"\\.\pipe\screensearch-v2-it-{role}-{}",
        Uuid::now_v7().simple()
    )
}

fn envelope(body: request_envelope::Body) -> RequestEnvelope {
    RequestEnvelope {
        request_id: Uuid::now_v7().to_string(),
        body: Some(body),
    }
}

async fn spawn(env: &TestEnv, model_root: &Path) -> (Child, WorkerLifeline) {
    let lifeline_name = unique_pipe("lifeline");
    let pending = create_worker_lifeline(&lifeline_name).expect("create lifeline pipe");
    let child = tokio::process::Command::new(&env.binary)
        .arg("--asset-root")
        .arg(&env.asset_root)
        .arg("--model-root")
        .arg(model_root)
        .arg("--pipe")
        .arg(&env.pipe)
        .arg("--lifeline-pipe")
        .arg(&lifeline_name)
        .spawn()
        .expect("spawn model worker");
    let lifeline = tokio::time::timeout(Duration::from_secs(10), pending.accept())
        .await
        .expect("worker connected its lifeline in time")
        .expect("accept worker lifeline");
    (child, lifeline)
}

async fn worker_health(pipe: &str) -> Option<WorkerHealthResponse> {
    let responses = IpcClient::new(pipe)
        .request(envelope(request_envelope::Body::WorkerHealth(
            WorkerHealthRequest {},
        )))
        .await
        .ok()?;
    responses
        .into_iter()
        .find_map(|response| match response.body {
            Some(response_envelope::Body::WorkerHealth(health)) => Some(health),
            _ => None,
        })
}

async fn wait_ready(pipe: &str, timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if let Some(health) = worker_health(pipe).await
            && health.status == "ready"
        {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

async fn collect_generation(pipe: &str, model_relative_path: &str) -> (usize, Option<String>) {
    let mut tokens = 0_usize;
    let mut status = None;
    IpcClient::new(pipe)
        .request_each(
            envelope(request_envelope::Body::WorkerGeneration(
                WorkerGenerationRequest {
                    model_id: "integration-test-gguf".to_owned(),
                    model_relative_path: model_relative_path.to_owned(),
                    prompt: GENERATION_PROMPT.to_owned(),
                },
            )),
            |response| {
                match response.body {
                    Some(response_envelope::Body::WorkerGeneration(event)) => match event.event {
                        Some(worker_generation_event::Event::Token(_)) => tokens += 1,
                        Some(worker_generation_event::Event::Completed(completed)) => {
                            status = Some(completed.status);
                        }
                        None => {}
                    },
                    Some(response_envelope::Body::Error(error)) => {
                        panic!("generation reported an error: {}", error.message)
                    }
                    _ => {}
                }
                Ok(())
            },
        )
        .await
        .expect("generation stream completes");
    (tokens, status)
}

async fn shutdown(mut child: Child, lifeline: WorkerLifeline) {
    let _ = child.start_kill();
    let _ = child.wait().await;
    drop(lifeline);
}

#[tokio::test]
#[ignore = "spawns the real model worker; set SCREENSEARCH_RUN_WORKER_IT=1"]
async fn health_ready_then_lifeline_closure_exits_worker() {
    require_opt_in();
    let env = TestEnv::new();
    let (mut child, lifeline) = spawn(&env, &env.model_root).await;

    assert!(
        wait_ready(&env.pipe, Duration::from_secs(20)).await,
        "worker must report ready"
    );

    drop(lifeline);
    let exited = tokio::time::timeout(Duration::from_secs(15), child.wait()).await;
    assert!(
        exited.is_ok(),
        "worker must self-exit once the daemon lifeline closes"
    );
    let _ = child.start_kill();
}

#[tokio::test]
#[ignore = "spawns the real model worker; set SCREENSEARCH_RUN_WORKER_IT=1"]
async fn kill_then_restart_recovers() {
    require_opt_in();
    let env = TestEnv::new();

    let (mut child, lifeline) = spawn(&env, &env.model_root).await;
    assert!(wait_ready(&env.pipe, Duration::from_secs(20)).await);
    child.start_kill().unwrap();
    let _ = child.wait().await;
    drop(lifeline);

    let (restarted, lifeline) = spawn(&env, &env.model_root).await;
    assert!(
        wait_ready(&env.pipe, Duration::from_secs(20)).await,
        "a freshly spawned worker must reclaim the pipe and report ready"
    );
    shutdown(restarted, lifeline).await;
}

#[tokio::test]
#[ignore = "spawns the real model worker; set SCREENSEARCH_RUN_WORKER_IT=1"]
async fn generation_round_trip_after_restart() {
    require_opt_in();
    let Some(gguf) = test_gguf() else {
        eprintln!("skipping: set SCREENSEARCH_TEST_GGUF to a .gguf file to run this case");
        return;
    };
    let model_root = gguf
        .parent()
        .expect("gguf has a parent directory")
        .to_path_buf();
    let filename = gguf
        .file_name()
        .and_then(|value| value.to_str())
        .expect("gguf filename is valid utf-8")
        .to_owned();
    let env = TestEnv::new();

    let (mut child, lifeline) = spawn(&env, &model_root).await;
    assert!(wait_ready(&env.pipe, Duration::from_secs(20)).await);
    child.start_kill().unwrap();
    let _ = child.wait().await;
    drop(lifeline);

    let (restarted, lifeline) = spawn(&env, &model_root).await;
    assert!(wait_ready(&env.pipe, Duration::from_secs(20)).await);
    let (tokens, status) = collect_generation(&env.pipe, &filename).await;
    assert!(tokens > 0, "generation must stream at least one token");
    assert_eq!(status.as_deref(), Some("answered"));
    shutdown(restarted, lifeline).await;
}

#[tokio::test]
#[ignore = "spawns the real model worker; set SCREENSEARCH_RUN_WORKER_IT=1"]
async fn cancel_mid_generation_keeps_health_responsive() {
    require_opt_in();
    let Some(gguf) = test_gguf() else {
        eprintln!("skipping: set SCREENSEARCH_TEST_GGUF to a .gguf file to run this case");
        return;
    };
    let model_root = gguf
        .parent()
        .expect("gguf has a parent directory")
        .to_path_buf();
    let filename = gguf
        .file_name()
        .and_then(|value| value.to_str())
        .expect("gguf filename is valid utf-8")
        .to_owned();
    let env = TestEnv::new();
    let (child, lifeline) = spawn(&env, &model_root).await;
    assert!(wait_ready(&env.pipe, Duration::from_secs(20)).await);

    let mut seen = 0_usize;
    let _ = IpcClient::new(&env.pipe)
        .request_each(
            envelope(request_envelope::Body::WorkerGeneration(
                WorkerGenerationRequest {
                    model_id: "integration-test-gguf".to_owned(),
                    model_relative_path: filename,
                    prompt: GENERATION_PROMPT.to_owned(),
                },
            )),
            |response| {
                if let Some(response_envelope::Body::WorkerGeneration(event)) = response.body
                    && let Some(worker_generation_event::Event::Token(_)) = event.event
                {
                    seen += 1;
                    return Err(IpcError::Handler("cancel".to_owned()));
                }
                Ok(())
            },
        )
        .await;
    assert!(
        seen >= 1,
        "at least one token should arrive before cancelling"
    );

    let started = Instant::now();
    assert!(
        worker_health(&env.pipe).await.is_some(),
        "health must respond after a cancelled generation"
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "health must not block behind a cancelled generation"
    );
    shutdown(child, lifeline).await;
}

#[tokio::test]
#[ignore = "spawns the real model worker; set SCREENSEARCH_RUN_WORKER_IT=1"]
async fn idle_unload_primitive_releases_resident_model() {
    require_opt_in();
    let Some(gguf) = test_gguf() else {
        eprintln!("skipping: set SCREENSEARCH_TEST_GGUF to a .gguf file to run this case");
        return;
    };
    let model_root = gguf
        .parent()
        .expect("gguf has a parent directory")
        .to_path_buf();
    let filename = gguf
        .file_name()
        .and_then(|value| value.to_str())
        .expect("gguf filename is valid utf-8")
        .to_owned();
    let env = TestEnv::new();
    let (child, lifeline) = spawn(&env, &model_root).await;
    assert!(wait_ready(&env.pipe, Duration::from_secs(20)).await);

    let (tokens, status) = collect_generation(&env.pipe, &filename).await;
    assert!(tokens > 0);
    assert_eq!(status.as_deref(), Some("answered"));
    assert!(
        worker_health(&env.pipe).await.unwrap().generation_loaded,
        "model must be resident after a generation"
    );

    let responses = IpcClient::new(&env.pipe)
        .request(envelope(request_envelope::Body::WorkerUnload(
            WorkerUnloadRequest {},
        )))
        .await
        .unwrap();
    assert!(
        responses.iter().any(|response| matches!(
            response.body,
            Some(response_envelope::Body::WorkerUnload(_))
        )),
        "raw worker unload must be acknowledged"
    );
    assert!(
        !worker_health(&env.pipe).await.unwrap().generation_loaded,
        "model must be released after a raw worker unload"
    );
    shutdown(child, lifeline).await;
}
