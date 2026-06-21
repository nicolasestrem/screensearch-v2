# ScreenSearch V2 Build Review

Review date: 2026-06-21  
Build reviewed: first truthful evidence-loop pass

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

## Deliberately skipped

- An approved and installed default GGUF generation model.
- Tray lifecycle and a system-wide search hotkey.
- Retention, exclusions, deletion, disk budget, model acquisition, signing, and production automation.

## Placeholder behavior that must not be mistaken for product behavior

- Fake providers remain for deterministic tests and are no longer composed by the production daemon.
- The model-worker executable remains a placeholder; real model work currently runs in the daemon.

## Existing strengths

- The modular-monolith shape is appropriate for a single-user desktop application.
- Persistence and IPC boundaries can survive replacement of adapters.
- The archive already treats OCR and embeddings as derived, versioned data.
- Safety policy is separated from native automation emission.

## Risks found

1. Full-frame exact hashes generate excessive unique captures for cursor, clock, animation, and other minor changes.
2. Capture and analysis share one daemon and need queue backpressure plus process isolation before production scale.
3. First model acquisition currently depends on Hugging Face connectivity and lacks a signed manifest flow.
4. The fixed 384-dimensional vector table is correct for MiniLM but requires a new migration for future dimensions.
5. The native model-worker boundary is declared but not exercised.
6. Current logging review has not yet proven that all future native errors are content-free.
7. Application exclusions and retention/deletion remain unimplemented; the UI discloses those gaps.

## Verification evidence

- `cargo fmt --all -- --check`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `npm run lint`
- `npm run build`

All commands above passed after the truthful evidence-loop implementation on 2026-06-21. The real Windows archive integration test also passed against a populated smoke archive; the long-running 10-million-capture benchmark remains explicitly ignored pending a dedicated run.

## Review verdict

The repository now has a credible truthful evidence loop: a smoke archive captured seven live screenshots, completed seven jobs automatically, and produced 393 positioned OCR blocks; a second archive passed a real semantic/evidence query against resolvable screenshot assets. It is not release-ready until privacy/backpressure policies, model-worker isolation, selected product UI, and release hardening land.
