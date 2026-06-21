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
- A system tray with at-a-glance capture state (tooltip + status line refreshed by a daemon health poll), Pause/Resume, Open, and Quit; closing the window hides it to the tray while the separate capture daemon keeps running (P2, native runtime pending manual Windows verification).
- A configurable system-wide summon hotkey (default `Ctrl+Shift+Space`) registered from Rust via `tauri-plugin-global-shortcut`, persisted as a shell-local JSON setting decoupled from the daemon archive, editable in Settings, and emitting a window-focus event to the UI.
- Complete keyboard navigation in the Memory Timeline UI: arrow/Home/End result selection with roving tab index and a typing guard, ARIA tablist arrow keys for evidence detail tabs, dialog focus trapping, Escape-to-close with focus restoration, and Ctrl+K search focus.

## Deliberately skipped

- An approved and installed default GGUF generation model.
- Locked-session detection, model acquisition, signing, factory reset of database/model files, and production automation.

## Placeholder behavior that must not be mistaken for product behavior

- Fake providers remain for deterministic tests and are no longer composed by the production daemon.
- The P3 branch changes the model-worker executable into the intended inference endpoint and routes daemon model ports through it, but this is not yet verified by the required checks in this session.

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

All commands above passed after the completed P1 pass on 2026-06-21. Coverage includes backpressure, pause precedence, exclusions, perceptual thresholds, settings validation/persistence, retention by age and asset budget, active-job protection, shared-asset reference handling, deletion cleanup recovery, queue/storage IPC, exact-text ranking boost, and model-revision isolation. A subsequent P0/P1 review pass added explicit analysis failure-path tests so retry backoff, dead-letter promotion at `MAX_JOB_ATTEMPTS`, persistence-level embedding-dimension rejection, and the `AnalysisService::process_one` dimension-mismatch → `fail_job` route are now covered (`cargo test -p screensearch-persistence`: 9 passed). The explicitly invoked ten-million-capture benchmark passed twice; the instrumented run populated 10,000,000 metadata rows in 90.73 seconds, produced a 3.62 GiB database, used 22.69 MiB peak process working set, and measured 38.1/108.6/136.7 microsecond p50/p95/p99 metadata queries. The final post-refactor populated local Windows archive test returned resolvable screenshot evidence with positioned bounds at 22.59/27.69/27.69 millisecond hybrid-search p50/p95/p99. See `docs/performance/P1_SCALE_BASELINE.md`.

The subsequent P2 product-shell pass (tray, configurable summon hotkey, keyboard navigation) re-ran `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (all green; the desktop binary compiles the tray/hotkey setup but `cargo test` never launches the GUI), `npm run lint`, and `npm run build`. Keyboard navigation was verified end-to-end with browser automation against the dev server: arrow/Home/End move result selection and focus, the typing guard prevents list navigation while the search field is focused, the evidence tablist responds to arrow keys, the settings dialog traps Tab focus and restores focus on Escape, the hotkey-capture control records combinations, and Ctrl+K focuses and selects the search field. The native tray, global hotkey, and hide-to-tray behavior still require a manual Windows `npm run tauri dev` session against a running daemon (spec §18) and are not yet confirmed; patch-plan item 14 stays open until then. The full shell reference and a step-by-step manual verification runbook are in `docs/design/p2-shell.md`.

## Review verdict

P0 remains verified and P1 is complete. The build has real revision-consistent hybrid retrieval, bounded capture growth, configurable local privacy/storage policy, durable deletion cleanup, indexing/storage metrics, and a reproduced ten-million-row engineering baseline. P2 product-shell work has landed in code and passes all automated checks: tray lifecycle, a configurable system-wide hotkey, hide-to-tray, and complete keyboard navigation are implemented and composed in production, with keyboard navigation verified by browser QA. It is not release-ready until the native tray/hotkey runtime is confirmed on Windows, plus model-worker isolation, generation/model decisions, security/packaging, and release-hardware soak validation land.

## P3 branch review note

The current `p3-model-selection-worker` branch adds generation-model catalog persistence, local GGUF import, explicit Hugging Face download, active-model selection, answer completion statuses, and worker IPC for OCR, embeddings, and generation. This note does not close P3: required Rust/frontend verification and live local-model benchmarking were blocked by the command-execution environment during implementation.
