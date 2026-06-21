# ScreenSearch V2 Build Review

Review date: 2026-06-21  
Build reviewed: completed P0 truthful evidence loop and P1 semantic-retrieval/scale phase

## Implemented

- Rust 2024 workspace with domain, ports, application, persistence, IPC, Windows adapter, model runtime, daemon, worker, and Tauri shell boundaries.
- Transactional capture/job persistence, content-addressed assets, leased jobs, retries, dead letters, FTS5, vectors, outbox events, and synthetic-scale harness.
- Protobuf request/response framing over a local Windows named pipe.
- Deterministic test providers proving capture, analysis, hybrid retrieval, citations, and token streaming without external models.
- React diagnostics UI proving daemon connectivity and IPC streaming.
- Automation approval, foreground, and emergency-abort policy tests without input emission.
- Windows CI for Rust/frontend verification and packaging.
- Real focused-monitor PNG capture with foreground application and title metadata.
- Automatic capture and durable analysis loops with serialized archive writes.
- Real Windows Media OCR with positioned evidence and user-profile language selection.
- Real quantized MiniLM ONNX embeddings cached locally and isolated by model revision.
- Evidence-rich IPC citations and authorized screenshot loading in the React diagnostics surface.
- A generic local GGUF llama.cpp generator adapter with evidence-only search as the safe default when no model is installed.
- The selected Memory Timeline product interface with real screenshot evidence, grouped results, filters, metadata/provenance tabs, privacy/settings dialogs, and visual QA.
- A real daemon-owned pause/resume capture control exposed through IPC and the desktop UI.
- Single-flight capture with queue high/low-water hysteresis and automatic recovery below low-water.
- Conservative perceptual-change filtering against the last accepted same-window frame, before asset persistence.
- Production self-exclusion plus case-insensitive application/title exclusion policy primitives.
- Queue depth, oldest pending age, retries, dead letters, capture state, and high-water visibility through IPC and the desktop status surface.
- Migration-backed archive settings for age retention, captured-asset budgets, and case-insensitive application/title exclusions.
- Live policy refresh after settings updates, periodic retention enforcement, transactional derived-data deletion, and durable idempotent cleanup of unreferenced assets.
- Storage metrics and confirmed captured-history deletion exposed through typed IPC, Tauri commands, and the selected Memory Timeline settings/privacy dialogs.
- Model-revision-isolated lexical and semantic ranking, deterministic exact-text boost, and a bounded exact-cosine path that avoids pathological vector-index startup on smaller live archives.
- An explicit ten-million-capture metadata benchmark with throughput, database size, query percentiles, peak process memory, and CPU time recorded on a named Windows machine.

## Deliberately skipped

- An approved and installed default GGUF generation model.
- Tray lifecycle and a system-wide search hotkey.
- Locked-session detection, tray lifecycle, a system-wide search hotkey, model acquisition, signing, factory reset of database/model files, and production automation.

## Placeholder behavior that must not be mistaken for product behavior

- Fake providers remain for deterministic tests and are no longer composed by the production daemon.
- The model-worker executable remains a placeholder; real model work currently runs in the daemon.

## Existing strengths

- The modular-monolith shape is appropriate for a single-user desktop application.
- Persistence and IPC boundaries can survive replacement of adapters.
- The archive already treats OCR and embeddings as derived, versioned data.
- Safety policy is separated from native automation emission.

## Risks found

1. The initial perceptual-change threshold needs measurement and tuning on the reference hardware; it deliberately favors preserving evidence over aggressive reduction.
2. Capture and analysis share one daemon; queue backpressure is implemented, but model process isolation remains necessary for release resilience.
3. First model acquisition currently depends on Hugging Face connectivity and lacks a signed manifest flow.
4. The fixed 384-dimensional vector table is correct for MiniLM but requires a new migration for future dimensions.
5. The native model-worker boundary is declared but not exercised.
6. Current logging review has not yet proven that all future native errors are content-free.
7. Long-duration capture CPU/storage growth and perceptual-threshold tuning remain release-hardening work beyond the completed P1 engineering baseline.

## Verification evidence

- `cargo fmt --all -- --check`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `npm run lint`
- `npm run build`

All commands above passed after the completed P1 pass on 2026-06-21. Coverage includes backpressure, pause precedence, exclusions, perceptual thresholds, settings validation/persistence, retention by age and asset budget, active-job protection, shared-asset reference handling, deletion cleanup recovery, queue/storage IPC, exact-text ranking boost, and model-revision isolation. The explicitly invoked ten-million-capture benchmark passed twice; the instrumented run populated 10,000,000 metadata rows in 90.73 seconds, produced a 3.62 GiB database, used 22.69 MiB peak process working set, and measured 38.1/108.6/136.7 microsecond p50/p95/p99 metadata queries. The final post-refactor populated local Windows archive test returned resolvable screenshot evidence with positioned bounds at 22.59/27.69/27.69 millisecond hybrid-search p50/p95/p99. See `docs/performance/P1_SCALE_BASELINE.md`.

## Review verdict

P0 remains verified and P1 is complete. The build has real revision-consistent hybrid retrieval, bounded capture growth, configurable local privacy/storage policy, durable deletion cleanup, indexing/storage metrics, and a reproduced ten-million-row engineering baseline. It is not release-ready until P2 product-shell lifecycle and keyboard work, model-worker isolation, generation/model decisions, security/packaging, and release-hardware soak validation land.
