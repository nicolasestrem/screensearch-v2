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

## 2026-06-21 — P2 product shell: tray, global hotkey, keyboard navigation

### Changed

- Added a system tray to the Tauri shell (`apps/desktop/src-tauri/src/main.rs`): an icon with a tooltip and a disabled status line that show at-a-glance capture state, plus **Open ScreenSearch**, **Pause/Resume capture**, and **Quit** items. A `tauri::async_runtime` background task polls daemon health every three seconds and updates the tooltip, status line, and pause/resume label; it reads `Daemon offline` when the daemon is unreachable.
- Made the window **hide to the tray** on close (`WindowEvent::CloseRequested` → `prevent_close` + `hide`); only tray **Quit** exits the shell, and the separate capture daemon is unaffected.
- Added a configurable system-wide summon hotkey (default `Ctrl+Shift+Space`) using `tauri-plugin-global-shortcut`, registered from Rust. Pressing it foregrounds and focuses the window and emits a `summon-search` event the UI listens for to focus and select the search field.
- Added a shell-local settings file (`apps/desktop/src-tauri/src/shell_settings.rs`): `ShellSettings { hotkey }` persisted atomically as JSON under the Tauri app-config directory, with `get_shell_settings`/`set_shell_settings` commands. `set_shell_settings` validates the accelerator before persisting and re-registers the shortcut live.
- Implemented complete keyboard navigation in the Memory Timeline UI (`apps/desktop/src/App.tsx`, `styles.css`): Arrow Up/Down/Home/End move the selected result (roving tab index, focus + scroll into view, guarded against firing while typing); ARIA tablist Arrow Left/Right/Home/End for the evidence detail tabs; a focus trap, first-control focus, Escape-to-close, and focus restoration for dialogs; a `:focus-visible` ring; and a hotkey-capture control in Settings that records combinations in Tauri's accelerator vocabulary. Ctrl+K continues to focus (and now selects) the search field.
- Added the `tray-icon` feature to `tauri`, plus `tauri-plugin-global-shortcut`, `serde_json`, and `tokio` to the desktop crate. Added `getShellSettings`/`setShellSettings` and a `summon-search` listener to `apps/desktop/src/api.ts`.
- Documented the slice comprehensively in `docs/design/p2-shell.md` (architecture, keyboard model, accelerator vocabulary, shell-local settings, and a step-by-step manual Windows verification runbook/checklist), linked from `docs/design/README.md` and referenced from the build review and patch plan.

### Fixed (UI resilience, surfaced during manual Windows testing)

- Hardened the timeline date helpers (`formatTime`/`formatDateTime`/`dayLabel` and the date filter) with a shared `safeDate` that returns a fallback for any unparseable timestamp instead of throwing `RangeError: Invalid time value` from `Intl.DateTimeFormat`. Previously a single malformed/edge `capturedAt` could crash `TimelineItem` and, with no boundary, blank the entire window. `safeDate` logs the offending raw value once (`console.warn`) for diagnosis.
- Added a React error boundary (`apps/desktop/src/ErrorBoundary.tsx`, wired in `main.tsx`) around the app so any unexpected render error shows a recoverable message instead of a blank window. (Error boundaries must be class components — the one intentional exception to the functional-component convention.)
- Confirmed against the live archive that real citation data is well-formed (valid RFC3339 millisecond timestamps and UUID chunk ids), so these are defense-in-depth guarantees, not a workaround for a data defect.

### Why

P2 turns the verified evidence/retrieval engine into a native-feeling utility: it should live in the tray, be summonable from anywhere, and be fully drivable by keyboard. The hotkey is a pure shell concern, so it is stored shell-locally rather than in the daemon archive, preserving the rule that the desktop shell never owns durable indexing state.

### Decisions made

- Default summon hotkey `Ctrl+Shift+Space` and **hide-to-tray** on window close were confirmed by the product owner; both are user-visible and the spec was otherwise silent.
- The global shortcut is registered from Rust (not the plugin's JS API), so no new capability/permission entries were required; custom commands and tray/window control already run under the granted `core:default`.

### Verification evidence

- `cargo fmt --all -- --check` — clean.
- `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- `cargo test --workspace` — all suites passed (the desktop binary compiles the tray/hotkey setup; `cargo test` does not launch the GUI).
- `npm run lint` and `npm run build` (`apps/desktop`) — clean.
- Browser keyboard-navigation QA against the dev server confirmed: arrow/Home/End selection with focus and detail-pane sync; the typing guard (Arrow keys ignored while the search field is focused); tablist arrow navigation; dialog focus trap (Tab wraps within the dialog); Escape closes the dialog and restores focus to the opener; the hotkey-capture control records `Ctrl+Shift+J` → `Ctrl Shift J`; and Ctrl+K focuses and selects the search field.

### Remaining boundary

Patch-plan item 14 stays **open**: the native tray, global hotkey, and hide-to-tray runtime still require a manual Windows `npm run tauri dev` session against a running daemon to satisfy the spec §18 "Verify tray pause/resume and global hotkey" check; `cargo test` only compiles the shell. Model-worker isolation, GGUF model selection, security/packaging, and release-hardware soak remain open.

## 2026-06-21 — P2 shell follow-ups from live Windows testing

### Fixed

- **Search citations rendered with `undefined` fields in the real shell** (timestamps, images, selection, and React keys all broken; one bad timestamp previously blanked the window). Root cause: `SearchUiEvent` in `apps/desktop/src-tauri/src/main.rs` used `#[serde(tag = "kind", rename_all = "camelCase")]`, but on a tagged enum `rename_all` renames only the variant tags, not the struct-variant fields — so real citations serialized with snake_case keys (`captured_at`, `chunk_id`, `capture_id`, `window_title`, `match_kind`, `ocr_model_id`, `embedding_model_id`) while the UI reads camelCase. Single-word fields (`application`, `score`, `excerpt`) matched, so it half-rendered; preview data (hand-written camelCase) hid it entirely. Fixed by adding `rename_all_fields = "camelCase"`, and locked with two `serde_json` contract tests asserting the citation/completed events emit camelCase keys and never snake_case. The instrumentation added in the prior entry (`safeDate` logging) surfaced the exact `undefined` value that pinpointed this.

### Added

- A visible **Search** submit button in the command bar next to the `Ctrl K` hint (`apps/desktop/src/App.tsx`, `styles.css`); Enter already submitted the form, but the click affordance was missing and Ctrl+K (focus) is not a discoverable submit on Windows. The button is `type="submit"`, disabled when the query is empty or a search is in flight.
- **Native notifications on capture pause/resume** plus an immediate tray refresh, so toggling capture from the tray gives feedback even when the window is hidden (`tauri-plugin-notification`, `notification:default` capability, `notify_capture_state`/`refresh_tray` in `main.rs`). The tray tooltip/status now update instantly on toggle rather than only on the 3-second poll. Windows toast notifications require a registered application id, so they appear reliably in packaged builds and may be suppressed in `tauri dev`; the tray tooltip/menu remain the dev-mode at-a-glance signal.

### Verification evidence

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` — all clean/green, including the two new `screensearch-desktop` serialization tests.
- `npm run lint` and `npm run build` (`apps/desktop`) — clean.
- Browser QA: the Search button is a form submit, disabled on empty query and enabled when filled; the preview UI renders with zero console warnings/errors.

### Remaining boundary

Unchanged: patch-plan item 14 stays open pending the manual Windows tray/hotkey runtime check (spec §18). The pause/resume notification is best confirmed in a packaged build.
