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
- A system tray with at-a-glance capture state (tooltip + status line refreshed by a daemon health poll), Pause/Resume, Open, and Quit; closing the window hides it to the tray while the separate capture daemon keeps running (P2, native runtime user-attested on Windows).
- A configurable system-wide summon hotkey (default `Ctrl+Shift+Space`) registered from Rust via `tauri-plugin-global-shortcut`, persisted as a shell-local JSON setting decoupled from the daemon archive, editable in Settings, and emitting a window-focus event to the UI.
- Complete keyboard navigation in the Memory Timeline UI: arrow/Home/End result selection with roving tab index and a typing guard, ARIA tablist arrow keys for evidence detail tabs, dialog focus trapping, Escape-to-close with focus restoration, and Ctrl+K search focus.
- Guarded automation V1 on the `codex/p4-guarded-automation` branch: domain policy, content-free persistence, daemon orchestration, typed protobuf/Tauri IPC, native Windows UIA/keyboard emission, manual approval UI, and a gated synthetic Windows fixture.
- Useful local-answer planning: deterministic local-time source/time filters for the supplied Telegram/GitHub/Codex/Amazon prompts, backend filtered hybrid search, OR fallback lexical retrieval, metadata-only retrieval, additive search-plan IPC, richer answer prompts with local timestamp/application/title metadata, UI `<think>` stripping, and guided Settings readiness/model setup.

## Deliberately skipped

- An approved and installed default GGUF generation model, qualitative groundedness scoring, and memory-pressure-triggered model unload.
- Model acquisition/release selection, signing, factory reset of database/model files, capture-side locked-session privacy handling, and release-hardening item 18.

## Placeholder behavior that must not be mistaken for product behavior

- Fake providers remain for deterministic tests and are no longer composed by the production daemon.
- The model-worker executable is the real inference endpoint for daemon OCR, embedding, and generation ports, and is supervised with bounded restarts plus a parent lifeline. Live Windows worker validation passed against two local GGUF candidates. GGUF generation can be built with opt-in Vulkan GPU offload and falls back to CPU when llama.cpp reports no supported backend. Release GGUF selection remains open.

## Existing strengths

- The modular-monolith shape is appropriate for a single-user desktop application.
- Persistence and IPC boundaries can survive replacement of adapters.
- The archive already treats OCR and embeddings as derived, versioned data.
- Automation policy, persistence, daemon orchestration, UI approval, and native Windows emission stay separated behind ports. Persistence/audit records and stable failure surfaces stay content-free; approval/execute IPC carries the reviewed typed plan.

## Risks found

1. The initial perceptual-change threshold needs measurement and tuning on the reference hardware; it deliberately favors preserving evidence over aggressive reduction.
2. Capture and analysis share one daemon; queue backpressure is implemented, but model process isolation remains necessary for release resilience.
3. First model acquisition currently depends on Hugging Face connectivity and lacks a signed manifest flow.
4. The fixed 384-dimensional vector table is correct for MiniLM but requires a new migration for future dimensions.
5. The native model-worker boundary is now supervised (bounded restarts + parent lifeline) and exercised by gated integration tests with local GGUF candidates; GAP-002/GAP-003 model decisions, qualitative answer scoring, and GAP-008 memory-pressure unload remain before item 15 can close.
6. Current logging review has not yet proven that all future native errors are content-free.
7. Long-duration capture CPU/storage growth and perceptual-threshold tuning remain release-hardening work beyond the completed P1 engineering baseline.
8. Guarded automation is intentionally default-off and narrowly scoped. Support policy, packaging/signing, and broader release-hardening remain before public release.

## Verification evidence

- `cargo fmt --all -- --check`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `npm run lint`
- `npm run build`

All commands above passed after the completed P1 pass on 2026-06-21. Coverage includes backpressure, pause precedence, exclusions, perceptual thresholds, settings validation/persistence, retention by age and asset budget, active-job protection, shared-asset reference handling, deletion cleanup recovery, queue/storage IPC, exact-text ranking boost, and model-revision isolation. A subsequent P0/P1 review pass added explicit analysis failure-path tests so retry backoff, dead-letter promotion at `MAX_JOB_ATTEMPTS`, persistence-level embedding-dimension rejection, and the `AnalysisService::process_one` dimension-mismatch → `fail_job` route are now covered (`cargo test -p screensearch-persistence`: 9 passed). The explicitly invoked ten-million-capture benchmark passed twice; the instrumented run populated 10,000,000 metadata rows in 90.73 seconds, produced a 3.62 GiB database, used 22.69 MiB peak process working set, and measured 38.1/108.6/136.7 microsecond p50/p95/p99 metadata queries. The final post-refactor populated local Windows archive test returned resolvable screenshot evidence with positioned bounds at 22.59/27.69/27.69 millisecond hybrid-search p50/p95/p99. See `docs/performance/P1_SCALE_BASELINE.md`.

The subsequent P2 product-shell pass (tray, configurable summon hotkey, keyboard navigation) re-ran `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (all green; the desktop binary compiles the tray/hotkey setup but `cargo test` never launches the GUI), `npm run lint`, and `npm run build`. Keyboard navigation was verified end-to-end with browser automation against the dev server. The native tray, global hotkey, hide-to-tray, hotkey persistence, tray pause/resume, Quit, and daemon-offline behavior were user-attested as passed in a manual Windows `npm run tauri dev` session against a running daemon on 2026-06-22. Patch-plan item 14 is closed. The full shell reference and completed manual verification checklist are in `docs/design/p2-shell.md`.

## Review verdict

P0 remains verified and P1 is complete. The build has real revision-consistent hybrid retrieval, bounded capture growth, configurable local privacy/storage policy, durable deletion cleanup, indexing/storage metrics, and a reproduced ten-million-row engineering baseline. P2 product-shell work is closed: tray lifecycle, a configurable system-wide hotkey, hide-to-tray, and complete keyboard navigation are implemented, composed in production, and verified by automated/browser QA plus user-attested native Windows runtime checks. It is not release-ready until generation/model decisions, security/packaging, and release-hardware soak validation land.

## P3 branch review note

The `p3-model-selection-worker` branch adds generation-model catalog persistence, local GGUF import, explicit Hugging Face download, active-model selection, answer completion statuses, and worker IPC for OCR, embeddings, and generation.

A 2026-06-22 review-and-harden pass made the worker boundary genuinely *supervised* (patch-plan item 16) and gave the generation model a real memory lifecycle (item 15), then ran the full required verification that the original implementation session could not:

- **Worker supervision.** The daemon now runs a supervisor task that detects worker exits and restarts the worker within a sliding-window bounded budget (`RestartPolicy`, unit-tested); exhausting the budget surfaces as a loud daemon exit. A per-instance parent **lifeline pipe** ties the worker's lifetime to the daemon, so a daemon crash no longer orphans a worker that would squat the worker pipe. Both restart budget/backoff and the lifeline use safe Rust and code constants — no new environment variables, no new `unsafe`.
- **Memory lifecycle.** An idle-timeout loop unloads the resident generation model after inactivity by issuing a raw worker unload that keeps the catalog selection intact, so the next query reloads lazily. A generation wall-clock deadline backstops cancellation, and `is_loaded()` is now lock-free so a health probe never blocks behind an in-flight generation. (Memory-pressure-triggered unload is deferred — see GAP-008.)
- **Revision integrity.** The daemon's worker client now verifies the worker-reported OCR/embedding revision against the expected id and fails loudly on drift, protecting the ADR 0002 invariant.
- **Tests.** New CI-run tests cover the catalog invariants, domain validation, every `answer_status` branch, the restart policy, and the deadline predicate. A new gated `apps/daemon/tests/worker_supervision.rs` (opt-in `SCREENSEARCH_RUN_WORKER_IT=1`, GGUF cases on `SCREENSEARCH_TEST_GGUF`) exercises readiness, lifeline-exit, kill→restart recovery, cancellation, and idle unload against the real worker process.

Verification on 2026-06-22 (Windows): `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all passed (52 passed, 7 ignored gated); `npm run lint` and `npm run build` passed in `apps/desktop`.

**Item 16 is closed by the 2026-06-22 P2/P3 validation pass.** The live gated worker suite passed with both `Ministral-3-3B-Reasoning-2512-Q4_K_M.gguf` and `NVIDIA-Nemotron3-Nano-4B-Q4_K_M.gguf`, covering readiness/lifeline, kill-then-restart recovery, generation after restart, cancellation health responsiveness, and idle unload. **Item 15 stays open** for the final model/legal/acquisition decisions in GAP-002/GAP-003, qualitative groundedness/citation scoring, the memory-pressure unload path in GAP-008, and release GPU packaging/validation where applicable. The smoke benchmark results are recorded in `docs/performance/P3_MODEL_SELECTION.md`; Vulkan build verification is currently blocked locally by the missing `VULKAN_SDK`.

## P4 branch review note

The `codex/p4-guarded-automation` branch implements patch-plan item 17 as a production path behind explicit default-off opt-in.

- **Specification and ADRs.** The master specification and ADR set now define the approved action schema, safety bounds, approval lifecycle, abort heartbeat, and privacy contract. `docs/design/p4-guarded-automation.md` is the durable design/runbook reference.
- **Domain and persistence.** `AutomationPlanV1`, `AutomationTarget`, `AutomationAction`, validation, canonical BLAKE3 plan digests, default-off settings, and the content-free v2 run ledger are implemented. Persistence stores only identifiers, plan digest, action count, lifecycle status, timestamps, expiry, and stable failure code.
- **Daemon and IPC.** `AutomationService` owns enablement, heartbeat freshness, one-shot approval, exact-plan verification, single-flight execution, abort latching/reset, timeout/rate gates, repeated foreground/session checks, terminal audit transitions, and restart recovery. Protobuf/Tauri operations cover status, enable, foreground target, approve, execute, abort, reset, and safety heartbeat.
- **Windows emission.** `WindowsAutomationPlatform` identifies targets by HWND/PID/executable, fails closed on lock/identity drift, resolves UIA controls uniquely by exact Automation ID below the approved HWND, supports Invoke and writable Value patterns, and uses `SendInput` for typed chords/text with UTF-16 encoding, modifier release, and partial-injection detection. Mouse, clipboard, shell, elevation bypass, and Windows-key actions are absent.
- **Manual UI.** The desktop adds a default-off guarded automation modal with warning confirmation, live abort state, target capture, ordered action editing, exact JSON review, separate approve/execute steps, execution result display, and explicit abort reset. Preview mode is non-emitting.
- **Verification coverage.** CI-run tests cover policy validation, digest stability, persistence privacy/default-off/approval lifecycle/recovery, daemon heartbeat/focus/session/abort/concurrency/timeout gates, protobuf/handler mappings, and Windows input encoding/identity behavior. A gated `crates/windows/tests/guarded_automation_fixture.rs` creates its own synthetic Win32 window and verifies UIA set-value, UIA invoke, UTF-16 text, key-chord fallback, and foreground rejection without controlling unrelated applications.

Item 17 is closed by this branch. P2/P3 open items and release-hardening item 18 remain open.
