# ScreenSearch V2 Project Context

Last verified: 2026-06-21

This document answers one question: **what is true today?** It does not select future implementation details.

## Repository status

ScreenSearch V2 is an independent, uncommitted Git repository on branch `main`. It is a Rust 2024 workspace with a Tauri 2 React/TypeScript desktop shell. The truthful evidence-loop pass compiles and its workspace tests, Rust formatting, Clippy checks, frontend lint, and frontend production build pass as of the date above.

## Current process boundaries

- `screensearch-daemon` owns the database, asset store, capture ingestion, job processing, search orchestration, and local named-pipe server.
- `screensearch-desktop` is a Tauri process. It proxies typed commands from React to the daemon.
- `screensearch-model-worker` exists as a process boundary declaration but does not yet host production model inference.
- Protobuf request/response envelopes are length-delimited over a Windows named pipe.

## Current workspace

- `crates/domain`: identifiers, captures, OCR blocks, indexed chunks, search hits, and streamed search events.
- `crates/ports`: capture, asset, archive, OCR, embedding, generation, and automation interfaces.
- `crates/application`: capture ingestion, durable analysis, prompt assembly, and streamed cited search orchestration.
- `crates/persistence`: content-addressed file assets and a libSQL archive.
- `crates/ipc`: protobuf contracts and named-pipe transport.
- `crates/windows`: production focused-monitor capture, Windows Media OCR, test doubles, and automation policy guards.
- `crates/model-runtime`: production MiniLM embeddings and llama.cpp generation plus deterministic test doubles.
- `apps/daemon`: adapter composition and IPC request handling.
- `apps/desktop`: Tauri commands and the React diagnostics screen.
- `apps/model-worker`: placeholder worker executable.

## Current data model

Migration `0001_initial.sql` creates:

- immutable content-addressed `asset` rows;
- `capture` metadata with exact-fingerprint uniqueness;
- leased and retryable `analysis_job` rows;
- versioned `embedding_model` rows;
- positioned `ocr_block` rows;
- `search_chunk`, FTS5, and 384-dimensional vector tables;
- `outbox_event`, `dead_letter`, and `automation_run` tables.

Migrations `0002` through `0004` make OCR confidence nullable for providers that do not expose it, connect search chunks to positioned OCR evidence, and register the active real embedding revision.

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
- Approval, foreground-window, and abort policy checks for automation.
- Focused-monitor Windows capture encoded as PNG through XCap.
- Automatic two-second capture scheduling and continuous durable-job draining.
- Offline Windows Media OCR using installed user language packs, including line bounds and language.
- Quantized `Xenova/all-MiniLM-L6-v2` ONNX embeddings through fastembed, cached below the app data directory and executed locally.
- Evidence-rich search citations containing screenshot, time, application, title, excerpt, match provenance, model revisions, and highlight bounds.
- Authorized screenshot retrieval by capture identifier and screenshot rendering in the diagnostics UI.
- The selected Memory Timeline product interface with search, client-side date/application filtering, grouped evidence, a screenshot inspector, OCR highlights, metadata/provenance tabs, and optional answer state.
- A real pause/resume control propagated over IPC to the daemon capture loop.
- A real llama.cpp GGUF adapter that is dormant until an explicitly installed model is selected; evidence-only search does not require it.

## What is fake or disabled

- Deterministic fake capture, OCR, embeddings, and generation remain available only for tests.
- No GGUF generation model is selected or installed by default; the diagnostics UI requests evidence-only search.
- The model-worker process does not perform inference.
- OS automation validates policy but emits no keyboard or mouse input.

## Current user experience

The React screen implements the product-owner-selected Memory Timeline direction. Capture and analysis are automatic; search renders chronologically grouped screenshot evidence with timestamps, application/window metadata, excerpts, provenance, and positioned OCR highlights. Search, date/application filters, evidence selection, detail tabs, pause/resume, privacy/settings dialogs, manual capture, and optional generation states are interactive. Tray lifecycle and a system-wide hotkey remain unimplemented.

## Current dependencies and infrastructure

- Rust 1.88+ and Node.js 22+.
- Local Windows WebView2 runtime.
- No backend service, cloud database, account system, or remote inference endpoint.
- First acquisition of the embedding model uses Hugging Face; inference is local after cache population.
- Windows GitHub Actions validates Rust and frontend builds and packaging.
- Runtime data defaults to `%LOCALAPPDATA%\ScreenSearchV2` and can be overridden by `SCREENSEARCH_DATA_DIR`.

## Known issues

1. Exact full-frame hashes do not handle visually insignificant changes or near-duplicates.
2. Queue backpressure, pause, exclusions, retention, deletion, and disk budgets are not implemented.
3. The daemon is not supervised or launched by the desktop shell.
4. No global hotkey or tray lifecycle exists yet; Ctrl+K currently focuses search only inside the window.
5. The model worker boundary is not yet exercised; OCR and embeddings currently execute in the daemon process.
6. The GGUF provider has no approved default model, manifest, acquisition flow, or packaged weights.
7. Automation policy is real, but native input emission remains disabled.
8. Capture cadence has not been tuned against CPU, storage growth, perceptual deduplication, or a 10-million-capture benchmark.
9. Named-pipe access-control hardening and release signing remain incomplete.

## Non-goals

The non-goals in `00_PROJECT_INTAKE.md` are binding. In particular, later agents must not introduce cloud services, accounts, V1 compatibility, autonomous actions, or multi-OS abstractions in the initial Windows release without a spec change.
