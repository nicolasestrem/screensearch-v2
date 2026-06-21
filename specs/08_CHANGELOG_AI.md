# ScreenSearch V2 AI-Assisted Changelog

This log records meaningful AI-assisted repository changes and their reasons. It is not a substitute for Git history.

## 2026-06-21 — Specification engineering baseline

### Changed

- Added the nine-file `/specs` pipeline described by Nicolas Estrem's “Spec engineering for AI-assisted delivery.”
- Captured repository truth, strategy, production contracts, execution guardrails, current build review, ordered patch plan, human-owned gaps, and this changelog.

### Why

The bootstrap proved plumbing but left future implementation agents free to invent product, persistence, privacy, model, and failure decisions. The specification set separates current truth from desired direction and installs a stop-at-ambiguity rule.

### Decisions made

- Keep the modular monolith plus isolated model-worker boundary.
- Treat visual evidence as the primary search output and generation as optional.
- Implement a Windows-only, offline, single-user release before expanding scope.
- Keep fake providers test-only once real adapters are composed.
- Defer production UI implementation until realistic evidence exists and one of three visual directions is selected.

### Not changed

- No production code, schema, IPC contract, dependency, or generated artifact changed in this specification pass.

### Source

- https://estrem.eu/dev/spec-engineering-for-ai-assisted-delivery

## 2026-06-21 — Truthful evidence loop

### Changed

- Replaced production fake capture with real focused-monitor PNG capture.
- Added continuous capture and durable analysis loops plus serialized archive writes.
- Replaced production fake OCR with Windows Media OCR and nullable confidence storage.
- Replaced production fake embeddings with quantized MiniLM ONNX inference through fastembed.
- Added positioned evidence metadata, authorized screenshot retrieval, and screenshot/highlight rendering.
- Added evidence-only search and a generic local GGUF llama.cpp provider without selecting a model.
- Added migrations `0002` through `0004` for OCR confidence, evidence linkage, and the active embedding revision.

### Why

The bootstrap UI could not demonstrate value because its pixels, OCR, vectors, and answer were fabricated. This pass establishes a verifiable screen-to-search path before production UI design.

### Verification evidence

- Clean smoke archive: 7 captures, 7 automatically completed jobs, 393 positioned OCR blocks.
- Real archive semantic/evidence integration test passed with resolvable image assets and positioned bounds.
- Rust formatting, workspace tests, warning-free Clippy, frontend lint, and frontend production build passed after the implementation.

### Remaining boundary

Production UI implementation pauses for selection among three visual directions. GGUF model selection, model-worker isolation, privacy/retention controls, and automation emission remain open.

## 2026-06-21 — Memory Timeline product interface

### Changed

- Recorded the product-owner selection of visual direction 2, Memory Timeline, as a durable design reference.
- Replaced the diagnostics scaffold with a compact search/timeline/evidence workspace using real screenshot assets and OCR bounds.
- Added interactive date/application filters, evidence selection, extracted-text/metadata/source tabs, privacy/settings dialogs, and optional local-answer state.
- Added a real IPC pause/resume command and wired it to the daemon's automatic capture loop.
- Added Phosphor icons and enlarged the default Tauri window for the dense evidence workspace.

### Why

The working evidence loop needed a product surface that made visual source material primary and exposed incomplete privacy/model capabilities truthfully.

### Verification evidence

- Frontend lint and production build passed.
- Browser interaction checks passed for search, filters, evidence selection, tabs, pause/resume, dialogs, and optional answer generation.
- Full-view and focused side-by-side comparison found no remaining P0/P1/P2 design issues; `design-qa.md` records `final result: passed`.

### Remaining boundary

Tray lifecycle, a system-wide hotkey, application exclusions, retention/deletion, model-worker isolation, GGUF model selection, and automation emission remain open.

## 2026-06-21 — P0 verification and P1 capture policy

### Changed

- Re-verified the full P0 Rust and frontend build rather than relying on the prior review claim.
- Serialized capture attempts and added 100/50-job high/low-water backpressure with automatic recovery.
- Added a conservative, deterministic perceptual-change filter that runs before asset persistence and preserves separate application/title evidence.
- Enforced production self-exclusion and added case-insensitive application/title policy primitives before asset persistence.
- Extended queue observability and IPC health with depth, oldest pending age, retries, dead letters, capture state, and high-water mark.
- Made policy skips explicit and content-free through the capture response and product status surface.
- Removed the archive path from the daemon-ready log event.

### Why

P1 semantic retrieval was already active, but capture could still grow the durable queue without bounds and store visually insignificant frames. This pass closes the first ordered P1 gap without inventing retention or user-configurable exclusion settings that belong to the next patch item.

### Verification evidence

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `npm run lint`
- `npm run build`
- The ignored real Windows archive integration test was explicitly run and passed against the populated local archive and cached MiniLM model.

### Remaining boundary

Performance acceptance and the explicit 10-million-row run remain pending a named reference machine. Persisted user exclusions, locked-session detection, retention, deletion, and disk budgets remain in the next privacy/storage slice.

## 2026-06-21 — P1 semantic retrieval and scale completed

### Changed

- Added migration-backed local archive settings for age retention, captured-asset budgets, and application/title exclusions.
- Applied updated exclusions to the live capture policy without restarting the daemon and enforced retention periodically and immediately after settings changes.
- Added storage metrics, transactional capture/derived-data deletion, active-job protection, shared-asset reference accounting, and durable idempotent cleanup of unreferenced image files.
- Exposed settings, storage status, privacy exclusions, and two-step captured-history deletion through protobuf, Tauri, and the selected Memory Timeline dialogs.
- Kept lexical and semantic results isolated to one embedding revision and added a bounded exact-cosine path for smaller live archives after the vector index showed pathological cold latency there.
- Reworked the explicit scale harness to populate and query ten million metadata rows without fabricating ten million image assets.
- Recorded the named-machine P1 baseline in `docs/performance/P1_SCALE_BASELINE.md`.

### Why

P1 requires semantic recall to remain truthful under growth. Revision consistency, capture shedding, retention, durable deletion, and measured scale are one system: each prevents the archive from becoming either misleading or operationally unbounded.

### Verification evidence

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `npm run lint`
- `npm run build`
- The explicitly opted-in ten-million-row benchmark passed twice on the named reference machine.
- The final post-refactor populated Windows archive test returned resolvable real screenshot evidence at 27.69 ms hybrid-search p95.
- Desktop settings/privacy dialogs passed interaction and layout QA at desktop and compact window sizes.

### Remaining boundary

P2 begins with tray lifecycle, the configurable system-wide hotkey, and complete keyboard navigation. Release hardening still includes locked-session detection, model-worker isolation, long-duration capture resource soak, factory-reset scope, named-pipe ACLs, signing, security checks, and packaging.

## 2026-06-21 — P0/P1 review and analysis failure-path test coverage

### Changed

- Audited the P0 evidence loop and P1 semantic-retrieval/scale phases against the patch-plan closing definition; confirmed real adapters are composed in production and no fake provider is reachable on the production path.
- Added four deterministic tests in `crates/persistence/src/lib.rs` covering analysis-pipeline failure behavior that was implemented but previously unexercised:
  - `fail_job_reschedules_with_backoff_and_defers_claim` — a failed job returns to `pending` with an incremented attempt and a deferred `next_run_at`, and is not immediately re-claimable.
  - `fail_job_dead_letters_after_max_attempts` — reaching `MAX_JOB_ATTEMPTS` promotes the job to `dead`, records a `dead_letter` row, and surfaces it through `queue_metrics().dead_letter_count`.
  - `complete_analysis_rejects_wrong_embedding_dimension` — a non-384-dimension embedding is rejected with `InvalidData` and the transaction rolls back, leaving the job claimable.
  - `process_one_fails_job_on_embedding_dimension_mismatch` — `AnalysisService::process_one` routes a dimension-mismatch error through `fail_job` (reschedule, not dead-letter) and propagates the error.

### Why

The two phases were substantively complete and truthful, but the analysis retry/dead-letter and dimension-rejection paths had no tests, so they did not meet the patch-plan bar that a closed item's tests cover both success and failure behavior. These tests close that gap without changing production code.

### Decisions made

- Placed the `AnalysisService::process_one` failure test in the persistence test module, which already dev-depends on `screensearch-application` and `screensearch-model-runtime`, rather than adding `application → persistence` as a dev-dependency (which would form a dev-dependency cycle). No production dependency direction changed.

### Verification evidence

- `cargo test -p screensearch-persistence` — 9 passed, 0 failed (5 prior + 4 new; scale and live-archive tests remain ignored).
- `cargo test --workspace` — all suites passed.
- `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- `cargo fmt --check` — clean.

### Not changed

- No production code, schema, IPC contract, dependency, or generated artifact changed. The change is test-only plus documentation.

### Remaining boundary

Unchanged from the prior P1 entry: P2 product-shell lifecycle and keyboard work, model-worker isolation, GGUF model selection, security/packaging, and release-hardware soak remain open.
