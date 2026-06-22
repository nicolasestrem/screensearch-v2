# ScreenSearch V2 Project Context

Last verified: 2026-06-22

This document answers one question: **what is true today?** It does not select future implementation details.

## Repository status

ScreenSearch V2 is an independent Git repository. It is a Rust 2024 workspace with a Tauri 2 React/TypeScript desktop shell. P0, P1, P2 shell runtime, supervised model-worker execution, and guarded automation are implemented through patch-plan item 17 except for item 15 model-selection release decisions and item 18 release hardening.

## Current process boundaries

- `screensearch-daemon` owns the database, asset store, capture ingestion, job processing, search orchestration, and local named-pipe server.
- `screensearch-desktop` is a Tauri process. It proxies typed commands from React to the daemon.
- `screensearch-model-worker` hosts production OCR, embedding, and generation inference behind the daemon, which supervises it with bounded restarts and a parent lifeline.
- Protobuf request/response envelopes are length-delimited over a Windows named pipe.

## Current workspace

- `crates/domain`: identifiers, captures, OCR blocks, indexed chunks, search hits, and streamed search events.
- `crates/ports`: capture, asset, archive, OCR, embedding, generation, automation platform, and automation repository interfaces.
- `crates/application`: capture ingestion, durable analysis, prompt assembly, and streamed cited search orchestration.
- `crates/persistence`: content-addressed file assets and a libSQL archive.
- `crates/ipc`: protobuf contracts and named-pipe transport.
- `crates/windows`: production focused-monitor capture, Windows Media OCR, test doubles, and automation policy guards.
- `crates/model-runtime`: production MiniLM embeddings and llama.cpp generation plus deterministic test doubles.
- `apps/daemon`: adapter composition, IPC request handling, and model-worker supervision.
- `apps/desktop`: Tauri commands and the React diagnostics screen.
- `apps/model-worker`: supervised OCR, embedding, and generation worker executable.

## Current data model

Migration `0001_initial.sql` creates:

- immutable content-addressed `asset` rows;
- `capture` metadata with exact-fingerprint uniqueness;
- leased and retryable `analysis_job` rows;
- versioned `embedding_model` rows;
- positioned `ocr_block` rows;
- `search_chunk`, FTS5, and 384-dimensional vector tables;
- `outbox_event`, `dead_letter`, and `automation_run` tables.

Migrations `0002` through `0004` make OCR confidence nullable for providers that do not expose it, connect search chunks to positioned OCR evidence, and register the active real embedding revision. Migration `0005_archive_policy.sql` adds the singleton archive settings record, application/title exclusions, and durable unreferenced-asset cleanup work. Migration `0007_guarded_automation.sql` adds default-off automation settings plus a content-free v2 automation approval/run ledger.

Capture metadata and an analysis job are committed transactionally. Analysis completion commits OCR blocks, chunks, embeddings, an outbox event, and job completion atomically. Jobs use leases and bounded retries.

## What is real

- Dependency boundaries and inward-facing ports.
- libSQL migrations and transactional persistence.
- Content-addressed filesystem storage.
- Exact byte-level capture deduplication.
- Durable job leasing, retry, dead-letter behavior, and transaction recovery tests.
- FTS5 and libSQL vector query paths with reciprocal-rank fusion.
- Protobuf framing and local named-pipe request/stream semantics.
- Tauri-to-daemon communication.
- Citation events followed by streamed token events.
- Guarded automation policy validation: 1–10 typed actions, 512-character bounds, no mouse/clipboard/shell actions, no Windows-key chords, 60-second one-shot approvals, 10-second execution deadline, and 100 ms pacing.
- Daemon-owned guarded automation orchestration with default-off settings, fresh abort heartbeat requirement, exact-plan BLAKE3 digest verification, single-flight execution, foreground/session checks before each action, abort latching/reset, restart recovery, and content-free audit transitions.
- Native Windows guarded automation emission through exact HWND/PID/executable target identity, WTS unlocked-session checks, unique UI Automation ID lookup beneath the approved HWND, Invoke/Value pattern support, and `SendInput` keyboard/text fallback with UTF-16 input, modifier release, and partial-injection detection.
- Typed protobuf and Tauri automation operations for status, enablement, foreground target capture, approval, execution, abort, reset, and safety heartbeat.
- Focused-monitor Windows capture encoded as PNG through XCap.
- Automatic two-second capture scheduling and continuous durable-job draining.
- Offline Windows Media OCR using installed user language packs, including line bounds and language.
- Quantized `Xenova/all-MiniLM-L6-v2` ONNX embeddings through fastembed, cached below the app data directory and executed locally.
- Evidence-rich search citations containing screenshot, time, application, title, excerpt, match provenance, model revisions, and highlight bounds.
- Authorized screenshot retrieval by capture identifier and screenshot rendering in the diagnostics UI.
- The selected Memory Timeline product interface with search, client-side date/application filtering, grouped evidence, a screenshot inspector, OCR highlights, metadata/provenance tabs, and optional answer state.
- A real pause/resume control propagated over IPC to the daemon capture loop.
- Serialized capture attempts with high/low-water queue backpressure, conservative perceptual-change filtering, and pre-persistence self-exclusion.
- Content-free queue health over IPC: depth, oldest pending age, retries, dead letters, high-water mark, and active/paused/backpressured state.
- Persisted application/title exclusions that update the live capture policy without a restart.
- Persisted age retention and captured-asset disk budgets, both disabled by the conservative `Keep all`/`No limit` default until the user chooses otherwise.
- Transactional capture/derived-data deletion plus durable, idempotent cleanup of unreferenced image assets.
- Settings and privacy controls for exclusions, retention, storage budget, storage metrics, and confirmed deletion of captured history.
- A reproducible ten-million-capture metadata benchmark and a live populated-archive hybrid-search latency check, recorded in `docs/performance/P1_SCALE_BASELINE.md`.
- A real llama.cpp GGUF adapter that is dormant until an explicitly installed model is selected; evidence-only search does not require it.
- A user-attested native Windows tray/hotkey runtime check for the P2 shell.
- A supervised model-worker production path with live gated validation against two local GGUF candidates; benchmark results are recorded in `docs/performance/P3_MODEL_SELECTION.md`.
- Opt-in Vulkan GPU offload for GGUF generation in the model worker, with runtime detection and CPU fallback when llama.cpp reports no supported GPU backend.

## What is fake or disabled

- Deterministic fake capture, OCR, embeddings, and generation remain available only for tests.
- No GGUF generation model is selected or installed by default; the UI can request evidence-only search and model management remains explicit.
- Guarded automation is disabled by default and never runs from generated plans. It emits native input only after manual enablement, target capture, exact review, one-shot approval, and same-plan execution while all safety gates still pass.

## Current user experience

The React screen implements the product-owner-selected Memory Timeline direction. Capture and analysis are automatic; search renders chronologically grouped screenshot evidence with timestamps, application/window metadata, excerpts, provenance, and positioned OCR highlights. Search, date/application filters, evidence selection, detail tabs, pause/resume, live privacy exclusions, retention/storage settings, confirmed captured-history deletion, manual capture, optional generation states, and the guarded automation approval workflow are interactive. The automation UI remains preview-safe outside Tauri and non-emitting until the native shell/daemon path is enabled.

## Current dependencies and infrastructure

- Rust 1.88+ and Node.js 22+.
- Local Windows WebView2 runtime.
- No backend service, cloud database, account system, or remote inference endpoint.
- First acquisition of the embedding model uses Hugging Face; inference is local after cache population.
- Windows GitHub Actions validates Rust and frontend builds and packaging.
- Runtime data defaults to `%LOCALAPPDATA%\ScreenSearchV2` and can be overridden by `SCREENSEARCH_DATA_DIR`.

## Known issues

1. The initial perceptual threshold is deterministic but still needs tuning against long-running real workloads; the P1 baseline deliberately favors preserving evidence.
2. The daemon is not supervised or launched by the desktop shell.
3. No approved default GGUF generation model, manifest, release acquisition policy, packaged weights, or release GPU packaging policy exist yet.
4. Qualitative generation scoring for groundedness, no-evidence refusal, conflict handling, prompt-injection resistance, and citation formatting is still pending.
5. Memory-pressure-triggered generation-model unload remains open; idle-timeout unload is implemented.
6. Guarded automation is included behind explicit default-off opt-in; it still needs release sign-off for user-facing scope and support policy.
7. Idle/active capture CPU, long-duration storage growth, and OCR/embedding queue latency still need release-hardware soak measurements; the P1 metadata-scale and live-search baselines are complete.
8. A full factory-reset scope covering archive database and model files is not implemented; confirmed deletion currently covers captured history and its derived evidence/assets.
9. Capture-side locked-session privacy handling remains open for release hardening; guarded automation has its own fail-closed WTS unlocked-session check.
10. Named-pipe access-control hardening and release signing remain incomplete.

## Non-goals

The non-goals in `00_PROJECT_INTAKE.md` are binding. In particular, later agents must not introduce cloud services, accounts, V1 compatibility, autonomous actions, or multi-OS abstractions in the initial Windows release without a spec change.
