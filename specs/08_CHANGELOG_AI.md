# ScreenSearch V2 AI-Assisted Changelog

This log records meaningful AI-assisted repository changes and their reasons. It is not a substitute for Git history.

## 2026-06-23 — P4 guarded-automation review hardening

### Fixed

- **Approval was unreachable through the UI (feature-breaking).** The daemon's `AutomationService::approve` runs the spec-mandated approval-time foreground-identity check (`check_target` → `matches_identity`). The capture and execute Tauri commands hide the ScreenSearch window so the external target is foreground during the daemon's checks, but `approve_automation` did not — so ScreenSearch was the foreground window when the user clicked Approve and every approval returned `target_changed`. Extracted the shared `with_foreground_yielded(app, fut)` helper (hide → 200 ms settle → request → show + focus) and routed capture, approve, and execute through it. Added a daemon-layer regression test `approval_requires_the_target_to_be_foreground_not_the_shell`.
- **Abort-shortcut fail-open.** The shell stored a one-time startup `register(...).is_ok()` snapshot in an `AtomicBool` that was never refreshed and only read by the heartbeat, so a later loss of the `Ctrl+Alt+Shift+Esc` registration (e.g. a summon-hotkey rebind, or OS reclaim) could still be reported to the daemon as live. `spawn_automation_heartbeat` now calls `ensure_abort_registered` each tick, which queries `is_registered` and re-registers if needed, stores the live truth, and notifies once on a transition to unavailable.
- **Digest bound a volatile field.** `AutomationPlanV1::canonical_digest` hashed the whole plan including `AutomationTarget.display_title`, which `matches_identity` deliberately excludes as volatile — so a window retitling itself between approve and execute would yield `plan_mismatch`. The digest now hashes an identity-only view (PID, HWND, lowercased executable) plus the ordered actions. Added `execution_accepts_title_only_changes` and a domain digest test.

### Changed

- **Diagnosable abort status.** `AutomationStatusResponse` gained `heartbeat_fresh` and `abort_registered` (additive proto fields, recomputed in `AutomationService::status` from a heartbeat snapshot that now stores the reported registration bit on every beat). The desktop renders a three-state pill — Live / Unavailable / Reconnecting — instead of one ambiguous "Unavailable".
- **Centralized key mapping (drift tripwire).** UI-token ↔ wire ↔ domain key/modifier conversion moved into `screensearch-ipc::convert`, shared by the daemon (`parse_automation_*`) and shell (`map_automation_*`). The token vocabulary is single-sourced in the domain (`AutomationKey::all`/`ui_token`, `KeyModifier::all`/`ui_token`) and the domain↔wire mappings are exhaustive, so a new key fails to compile until handled. A new `crates/ipc/tests/automation_keymap.rs` round-trips every variant.

### Why

A scrupulous Phase 4 ("guarded automation") re-review of the closed feature found that, despite faithful domain/persistence/IPC/native layers, the approval workflow was broken end-to-end (no test exercised the real approve→daemon foreground path), plus three correctness/robustness gaps. The user approved the fullest remediation scope (all findings, including the proto status fields and the IPC-crate conversion relocation). No production safety invariant was weakened; the daemon still fails closed on every gate.

### Verification

- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (all pass; gated native/scale tests remain `#[ignore]`), `npm run lint`, `npm run build` — verbatim output in the PR.
- The end-to-end approve→execute path and the abort-pill tri-state remain user-attested on Windows (they cannot run in CI); the runbook step is in `docs/design/p4-guarded-automation.md`.

### PR #17 review follow-ups

Addressed the Gemini / Codex / Claude review comments on the PR:

- **Window left hidden on cancellation/panic (Gemini, high).** `with_foreground_yielded` restored the window with explicit `show()`/`set_focus()` after the operation, so a future dropped at an `.await` (task cancellation) or a panic could leave the window hidden indefinitely. Restoration now runs from a `WindowRestoreGuard`'s `Drop`, guaranteeing it across normal completion, cancellation, and unwind.
- **Abort re-registration trusted a cache, not the OS (Codex, P1 — the important one).** `ensure_abort_registered` short-circuited on `manager.is_registered(...)`, but in `tauri-plugin-global-shortcut` 2.3.2 `is_registered` only consults the plugin's internal `shortcuts` map, not the OS — so a hotkey the OS silently reclaimed (while the cache entry remained) was never re-registered and the heartbeat kept reporting `abort_registered=true`. It now `unregister`s then `register`s each tick; `register` performs the real `RegisterHotKey`, so its result is OS truth (registering an already-OS-held hotkey fails, which is why the prior `unregister` is needed). The re-registered shortcut keeps dispatching through the plugin's global handler, so abort behavior is unchanged.
- **Digest was modifier-order-sensitive (Claude, optional).** A key chord's modifiers are a set, but the digest hashed them in caller order, so a non-UI caller listing `[Shift, Control]` at execute after `[Control, Shift]` at approval would hit `plan_mismatch`. The canonical digest now sorts a chord's modifiers; the stored/displayed plan keeps the reviewed order. Covered by `automation_digest_ignores_modifier_order`.
- Reviewers confirmed F1–F4 otherwise correct and fail-closed. The `Future` trait bound compiles via the Rust 2024 prelude (no explicit import needed); `AutomationKey::all()` remaining a hand-maintained array is an accepted, test-guarded trade-off.

Verified after the follow-ups: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` — all pass.

## 2026-06-23 — P3 constrained-synthesis hardening

### Fixed

- **Memory-pressure model unload (GAP-008, spec §11).** The resident generation model previously unloaded only on an idle timeout, but spec §11 requires unload on "an idle timeout **or** a memory-pressure signal." Added a safe `MemoryPressureMonitor` in `crates/windows` wrapping `CreateMemoryResourceNotification(LowMemoryResourceNotification)` + `QueryMemoryResourceNotification` (all `unsafe` FFI confined to that crate, exposed as `Result<bool, PortError>`). The daemon's existing `idle_unload_loop` now queries it each tick and unloads via the same raw worker-unload path when Windows signals low memory, logging `reason = "memory_pressure" | "idle"`. The decision is a pure, unit-tested `should_unload_generation`; no new env vars (the OS notification is the threshold). The live low-memory path is gated (`#[ignore]`) and needs manual Windows attestation.
- **Prompt injection hardening (GAP-011).** `assemble_prompt` wrote untrusted `application`, `window_title`, and OCR text into the answer prompt with raw `writeln!`, guarded only by a textual "untrusted evidence" instruction. Added `sanitize_untrusted_field`: control characters (including CR/LF/Tab) collapse to single spaces and `[`/`]` are rewritten to fullwidth U+FF3B/U+FF3D, so an adversarial window title can neither start its own prompt line (a forged field or instruction) nor forge a `[capture-id]` citation. Genuine citation lines use the daemon-emitted `capture_id` and are untouched. Covered by `prompt_neutralizes_adversarial_metadata` and a direct helper test.
- **Stale documentation.** `06_PATCH_PLAN.md` claimed the stored `context_tokens` was 2048; the live constant is `GENERATION_CONTEXT_TOKENS = 4096`, which the daemon stamps on download. Corrected the note and updated the GAP-008/GAP-011 status in `07_KNOWN_GAPS.md`.

### Why

A scrupulous Phase 3 ("constrained synthesis") review found the generation plumbing, worker supervision, idle unload, citations, and no-evidence path genuinely implemented, but two engineering gaps the specs themselves flagged open (GAP-008 memory-pressure unload, GAP-011 metadata prompt hardening) were unaddressed, plus a documentation-truthfulness drift. Model approval/acquisition (GAP-002/GAP-003), code signing (GAP-005), qualitative groundedness scoring, and release hardening (item 18) remain product/legal decisions and are out of scope. The model-revision-isolation invariant was confirmed already covered by `hybrid_ranking_boosts_exact_text_and_excludes_other_model_revisions`, so no duplicate test was added.

### Verification

- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` — see the PR for verbatim output.
- The live Windows memory-pressure probe (`cargo test -p screensearch-windows -- --ignored`) and the full GUI paths remain user-attested (they cannot run in CI).

### PR #16 review follow-ups

Addressed the Claude / Codex / Gemini review comments on the PR:

- **Chat-template special-token escaping (Codex P2 — the important one).** `sanitize_untrusted_field` defanged newlines and `[`/`]` but left model special-token delimiters such as `<|im_end|><|im_start|>system` intact; the llama.cpp path tokenizes the assembled prompt with `parse_special = true` (`str_to_token`, `crates/model-runtime/src/lib.rs`), so captured text could be read as role/control tokens. The sanitizer now inserts a zero-width space after every `<`, breaking `<|…|>` / `<…>` special-token strings while preserving the visible characters. Covered by `sanitize_untrusted_field_breaks_chat_template_delimiters` and an extended `prompt_neutralizes_adversarial_metadata`.
- **Observability of memory-pressure queries (Gemini + Claude).** `idle_unload_loop` previously discarded `is_low_memory()` errors with `.ok()`, so a permanently-broken `QueryMemoryResourceNotification` would silently disable the feature. It now logs a `warn!` on query failure and still falls back to "no pressure."
- **Idle clock measured from generation start (Gemini).** `WorkerModelClient::generate` stamped `last_generation` only at the start of a request, so a long generation shortened the effective idle window. It now also refreshes the timestamp as tokens stream and once the stream completes, so the idle-timeout unload measures from the end of activity.
- **Allocation in the sanitizer (Gemini).** Rewrote `sanitize_untrusted_field` as a single pass with one `String` allocation instead of an intermediate `String` plus a `Vec<&str>` join.
- **Unicode bidi/format overrides (Claude, non-blocking).** Documented as a known limitation in the helper's doc comment: characters like U+202E are not neutralized because they do not change a local model's token-level reading; tracked under GAP-011 as display-hardening.

Verified after the follow-ups: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (123 passed, 0 failed, 10 ignored gated) — all clean.

## 2026-06-23 — P2 product-shell review remediation

### Fixed

- **OCR overlay alignment (P1, core evidence feature).** OCR highlight overlays were positioned by percentage of the `.capture-image` container, but the `<img>` uses `object-fit: contain` inside fixed-aspect CSS boxes (thumbnail 16:9, large ~16:8.1 with a `max-height: 49vh` clamp). For any capture whose aspect ratio differed from the box (ultrawide, portrait/rotated monitors, or once the clamp engaged) the image letterboxed but the overlays stayed anchored to the full container, so the boxes drifted off the text. Added a `useContainedRect` hook (`useLayoutEffect` + `ResizeObserver`, content-box measurement, `object-fit: contain` math) and positioned overlays in pixels relative to the actually-rendered image rect. Verified live in the browser: the large detail box clamped to aspect 2.046 against the 2.39 fixture applies a 31.6 px top letterbox and overlays land on text; the thumbnail applies an 8.64 px letterbox; both track across viewport resizes.
- **Dialog focus trap + keyboard conflict (a11y).** The guarded-automation dialog focused its first control but never trapped `Tab` (the settings dialog already trapped it), so `Tab` could escape to the page behind — contrary to the documented "dialogs trap Tab focus." Extracted the trap into a shared `useDialogFocusTrap` hook used by both dialogs. Also added `event.stopPropagation()` to the detail-tab `onKeyDown` so the global window handler no longer also moves the timeline selection on `Home`/`End`, and `event.preventDefault()` to the Escape-to-blur branch. Made `aria-current` explicit (`"true"`).

### Changed

- Reworked the desktop preview fixture (`api.ts`): the synthetic citation now declares a 2600×1088 ultrawide asset with bounds over real text, so `npm run dev` / browser QA exercises overlay alignment instead of drawing boxes on blank space (the gap that let the original defect ship). The preview asset is a committed self-contained SVG (see the PR #15 review follow-up below) rather than a gitignored real capture.
- Added concise `///` doc comments to the desktop Tauri command boundary in `apps/desktop/src-tauri/src/main.rs` (maintainability; the binary crate does not trip `missing_docs`).

### Why

A scrupulous re-review of the closed P2 product shell against the specs found the shell solid but caught one real defect in the screenshot-grounded evidence feature — OCR overlays misaligned on non-16:9 captures — which escaped QA because the browser QA used synthetic bounds unrelated to text and the manual runbook never checked alignment. Two agent-flagged items were rejected after verification: `aria-current={selected || undefined}` is valid (React stringifies `true`), and "missing docs on Tauri commands fails clippy" is false (the desktop crate is a binary, so `missing_docs` never fires).

### Verification

- `npm run lint`, `npm run build` (apps/desktop) — clean.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (52 passed, 7 gated `#[ignore]`d) — clean.
- Live browser QA (Playwright against the dev server): overlay alignment on the large detail and thumbnail with the letterbox offset applied; `Home`/`End` on a focused detail tab moved the tab but not the timeline selection; `Tab` wrapped inside the automation dialog.
- Native Windows tray/hotkey and the new non-16:9 / portrait overlay check remain user-attested (the Tauri GUI cannot run in CI).

### PR #15 review follow-up

Addressed the Claude / Codex / Gemini review comments on the PR:

- **Codex (P2) — preview asset was missing.** The preview fetched `/qa-capture.png`, but that file is **gitignored** (it is a real screen capture, which the project must never commit), so on any other clone the fetch 404s and the new bounds never exercise alignment. Replaced the fetch with a committed, self-contained **synthetic SVG fixture** (`previewCaptureSvg` in `api.ts`, 2600×1088 ultrawide with text drawn under the citation bounds), returned as `image/svg+xml`. No binary asset, no user screen data, and preview/QA never 404s.
- **Gemini (medium) + Claude — focus-trap robustness.** Hardened `useDialogFocusTrap`: the focusable query now adds `summary` (so `<details>` boundaries are trapped), and filters to elements that can actually receive Tab focus — enabled, `tabIndex !== -1`, non-hidden inputs, and currently rendered (`getClientRects().length > 0`, which drops `display:none` elements). Added escaped-focus recovery (`!root.contains(activeElement)` → focus first). Verified the settings dialog (which contains a `<details>`) and automation dialog still trap and wrap `Tab` first↔last.
- **Claude (cosmetic).** Documented why `ref` stays in the `useContainedRect` dependency array (stable identity; satisfies exhaustive-deps without causing re-runs).

Re-verified: `npm run lint` / `npm run build` clean; live Playwright browser QA re-confirmed overlay alignment against the new SVG fixture (large + thumbnail, letterbox offset applied) and the settings/automation dialog focus traps. Rust gates unchanged (no Rust edits in this follow-up).

## 2026-06-23 — Phase 0→1 review remediation

### Changed

- **F1 — retry backoff jitter (spec §6).** Replaced the deterministic `2^min(attempt,8)` backoff with bounded "equal jitter" (`base/2 + (hash(job id, attempt) mod (base/2 + 1))`), keeping the exponential cap and a ≥1s floor. The offset is derived from BLAKE3 so it is reproducible (testable) and needs no runtime RNG dependency.
- **F4 — OCR text normalization (spec §7.2).** `WindowsOcrEngine` now collapses CR/CRLF to LF and applies Unicode NFC to each block's text before persistence, so stored OCR text, FTS terms, and chunk embeddings share one canonical representation. Added a pure `normalize_ocr_text` helper + unit test (`unicode-normalization` dependency).
- Refactored the libSQL migration runner into a single `apply_migration_if_absent` versioned-gate helper (also keeps `migrate()` under the clippy line limit) while adding migration `0008`.

### Added

- **F3 — embedding manifest (spec §8.1).** Migration `0008_embedding_model_manifest.sql` adds nullable manifest columns to `embedding_model` and records the active MiniLM revision (`751bff37…`), tokenizer revision, mean pooling, L2 normalization, Apache-2.0 license, and source URL. `FastEmbedEngine::manifest()` single-sources the same values and now backs `model_id()`/`dimensions()`; a persistence test asserts the DB row matches the runtime manifest field-for-field.
- **F6 — missing §18 integration tests.** Added expired-lease recovery, end-to-end queue saturation/backpressure resume (through `IngestService` against a real archive), and capture-commit orphan handling.
- **F6 orphan handling (spec §5).** Added `FileAssetStore::sweep_orphans` + `LibSqlArchive::referenced_asset_hashes`, wired into the daemon maintenance loop with a one-hour grace window. A frame whose asset file was written before its capture row failed to commit is now reconciled and removed; referenced and recently written files are never deleted. The sweep stays inside the content-addressed asset root, so it cannot touch the database or model files.

### Why

A scrupulous review of the completed P0 (truthful evidence loop) and P1 (semantic retrieval & scale) phases against `03_MASTER_PRODUCTION_SPEC.md` found the implementation faithful overall, with a few literal spec deviations (no backoff jitter, no OCR normalization, an incomplete embedding manifest), three §18-required integration tests absent, and the §5 file-orphan cleanup unimplemented. These are the low-risk, in-scope fixes; capture-side locked-session privacy (F5) remains a disclosed item-18 deferral, and the fixed/non-renewable analysis lease (F2) plus the diagnostic `processJobs` surface (F7) are recorded findings left out of this pass.

### Verification

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` — all pass (persistence 23, model-runtime 24, windows 5; gated suites remain `#[ignore]`d).

### PR #14 review follow-up

Addressed the Claude / Codex / Gemini review comments on the remediation PR:

- **Orphan sweep hardened.** Reworked the sweep to be candidate-first: `FileAssetStore::collect_orphan_candidates` walks the asset root, skips symlinked shards/files via `DirEntry::file_type` (root-confinement, per Codex), tolerates entries that vanish mid-walk (per Gemini/Claude), and returns only aged-out files; `LibSqlArchive::referenced_asset_hashes_among` then checks just those hashes in bounded `IN (...)` batches instead of loading every asset hash into memory (per Claude's 10M-scale note). `remove_files` skips locked/missing files instead of aborting the sweep.
- **Capture/sweep race closed.** `FileAssetStore::put` now refreshes a reused file's modification time, so a re-captured orphan is protected by the grace window until its new capture row commits (Codex P2). Added a regression test.
- **Sweep throttled** to once per hour in the maintenance loop rather than every 60 s tick (Gemini).
- **Embedding manifest provenance clarified** (Codex P1): documented in the manifest type, migration `0008`, and GAP-002 that `revision_hash` is the *advertised* upstream revision — `fastembed` downloads `Xenova/all-MiniLM-L6-v2`'s `main` branch unpinned, so within-archive isolation is enforced by `model_id` while hard pinning/verification is a model-acquisition decision (GAP-002/GAP-003). Noted that `revision_hash` and `tokenizer_revision` coincide for MiniLM (Claude nit).

### Why

The review surfaced one P1 (manifest could over-claim a download-verified revision), two P2s (a narrow capture/sweep race and a symlink root-confinement gap), and scalability/resilience notes on the orphan sweep. All are addressed in code except the model-acquisition decision behind the P1, which is correctly owned by GAP-002/GAP-003.

## 2026-06-22 — PR #12 review follow-up

### Changed

- Capped per-hit OCR excerpts before they enter answer prompts and added a regression test for long OCR chunks.
- Included every returned citation in answer prompts so generated answers and the visible evidence list use the same evidence set.
- Replaced the odd full-day helper call pattern with a dedicated local-midnight helper.
- Kept browser pages eligible by applying source filters to any OCR chunk in the same capture as well as app/title metadata, and documented the strict-extraction/loose-matching boundary.
- Removed the unavailable display-name requirement from the advanced Hugging Face download button.
- Made desktop `<think>` stripping hide content from the first unclosed streaming tag onward.
- Recorded known gaps for source vocabulary expansion, client timezone propagation through search IPC, and further prompt hardening for application/window-title metadata.
- Preserved Unicode alphanumeric terms during deterministic query planning so accented names and titles continue into retrieval and embeddings.
- Restricted afternoon time planning to the supported "this afternoon" anchor so unsupported modifiers such as "yesterday afternoon" do not accidentally receive today's filter.

### Why

PR review found one user-visible Settings bug, one prompt-context overflow risk, two planner recall regressions, and several design-limit notes that should be explicit for future work.

## 2026-06-22 — Useful local answers and guided settings

### Changed

- Added deterministic local-time query planning for the supplied Telegram, GitHub PR, Codex settings, and Amazon book prompts. The planner extracts source hints, bounded Windows-local time windows, and a reduced retrieval query.
- Added domain `SearchFilters`, `SearchPlan`, and `SearchOptions`, plus an additive search-plan event over protobuf/Tauri/TypeScript so the UI can show the interpreted plan.
- Changed hybrid search so source/time metadata filters apply inside lexical and semantic candidate SQL before rank fusion. Filtered semantic search uses the exact vector path rather than global top-k.
- Replaced strict all-term FTS with phrase/exact boosting plus OR fallback, preserving capture-level dedupe and embedding model-revision isolation.
- Enriched answer prompts with capture id, local timestamp/timezone, application, window title, and OCR excerpt, and explicitly instructs the model that OCR is untrusted evidence requiring citations or uncertainty.
- Hid generated `<think>` spans before desktop display.
- Redesigned Settings around answer readiness, timezone basis, active/installed answer model state, blank guided local GGUF import, advanced explicit HF download, explicit retention/budget state, and a conservative storage reset.
- Added a content-free ignored local archive smoke for the four acceptance prompts.

### Why

The app previously searched raw text with post-hoc UI filtering and sparse answer context, so questions like "Telegram around noon" or "largest PR today" could return irrelevant or uncited results. This pass makes the local evidence constraints deterministic, inspectable, and enforced before ranking while keeping external services out of answer generation.

### Remaining boundary

Patch-plan item 15 stays open for qualitative answer scoring, release model approval/acquisition, and memory-pressure unload. Item 18 stays open for locked-session privacy, security, packaging/signing, and release soak.

## 2026-06-22 — PR #10 review follow-up

### Changed

- Replaced the GGUF GPU offload request sentinel with an explicit `i32::MAX as u32` constant so full layer offload no longer relies on `llama-cpp-2` clamping an overflowing `u32::MAX` input.
- Added a unit assertion documenting the representable offload limit used by `LlamaModelParams::with_n_gpu_layers`.
- Corrected stale Claude guidance so future changes update `CHANGELOG.md` and `specs/08_CHANGELOG_AI.md` when each is relevant.

### Why

PR #10 review identified that `u32::MAX` worked only because the current dependency clamps it internally. Passing the largest value the API can represent directly is clearer and avoids a future runtime panic if the dependency changes its conversion behavior.

## 2026-06-22 — P2/P3 validation closeout

### Changed

- Closed patch-plan item 14 from user-attested manual Windows verification of the native tray, pause/resume, hide-to-tray, global summon hotkey, hotkey persistence, tray Quit, and daemon-offline behavior.
- Ran local text-only GGUF smoke benchmarks against `Ministral-3-3B-Reasoning-2512-Q4_K_M.gguf` and `NVIDIA-Nemotron3-Nano-4B-Q4_K_M.gguf`.
- Ran the gated live model-worker supervision suite against both local candidates. Each run passed readiness/lifeline, kill-then-restart recovery, generation after restart, cancellation health responsiveness, and idle unload.
- Recorded the measurements and release-selection boundaries in `docs/performance/P3_MODEL_SELECTION.md`.
- Added opt-in Vulkan GPU offload for GGUF generation in the model worker. The runtime requests full llama.cpp layer offload only when `supports_gpu_offload()` reports support, and otherwise keeps the CPU fallback path.
- Closed patch-plan item 16 because production OCR, embeddings, and generation now run behind the supervised worker boundary and the live worker path is validated.

### Why

Items 14 and 16 were implemented but still open for runtime evidence. This pass records that evidence and narrows item 15 to the remaining model-release decisions, qualitative answer scoring, and memory-pressure unload policy.

### Verification evidence

- `cargo build -p screensearch-model-worker` passed.
- `screensearch-model-worker.exe --benchmark-model` passed for both local GGUF candidates.
- `SCREENSEARCH_RUN_WORKER_IT=1 cargo test -p screensearch-daemon --test worker_supervision -- --ignored --nocapture` passed for both local GGUF candidates.
- `cargo test -p screensearch-model-runtime` passed for the GPU-parameter selection helper and existing runtime tests.
- `cargo build -p screensearch-model-worker --features gpu-vulkan` is blocked on this machine because the Vulkan SDK is not installed (`VULKAN_SDK` is `NotPresent`).

### Remaining boundary

Patch-plan item 15 remains open. GAP-002/GAP-003 still own release model approval and acquisition policy; GAP-008 still owns memory-pressure-triggered unload; qualitative grounded answer scoring and release GPU packaging/validation remain pending.

## 2026-06-22 — P4 guarded automation

### Changed

- Specified Guarded Automation V1 in the master spec, ADRs, and `docs/design/p4-guarded-automation.md`: typed actions, safety bounds, approval lifecycle, abort heartbeat, foreground/session checks, and content-free privacy rules.
- Added domain policy for `AutomationPlanV1`, `AutomationTarget`, `UiaInvoke`, `UiaSetValue`, `KeyChord`, and `TypeText` with limits for action count, field length, unsupported action classes, Windows-key exclusion, approval TTL, execution timeout, and pacing.
- Added migration `0007_guarded_automation.sql` with default-off settings and a v2 content-free approval/run ledger. The ledger stores only identifiers, canonical BLAKE3 plan digest, action count, status, timestamps, expiry, and stable failure code.
- Added an `AutomationRepository` port and libSQL implementation for settings, approvals, atomic run claims, terminal transitions, and interrupted-run recovery.
- Added daemon-owned `AutomationService` plus typed protobuf/Tauri operations for status, enablement, foreground target capture, approval, execution, abort, reset, and safety heartbeat.
- Implemented native Windows automation through exact HWND/PID/executable identity, WTS unlocked-session checks, unique UIA Automation ID lookup under the approved HWND, Invoke/Value patterns, and `SendInput` keyboard/text fallback with UTF-16 input, modifier release, and partial-injection detection.
- Added the desktop guarded automation modal with warning confirmation, abort-state visibility, target capture, ordered action editing, exact JSON review, separate approve/execute steps, execution result display, and abort reset. Preview mode remains non-emitting.
- Added an opt-in synthetic Win32 fixture for native automation verification. The fixture creates and controls only its own test window.

### Why

Patch-plan item 17 required a real but tightly guarded automation path. This implementation keeps automation out of autonomous/model-generated flows and exposes it only through explicit user enablement, target capture, exact review, one-shot approval, and repeated safety checks.

### Verification evidence

- Focused domain, persistence, application, IPC, daemon, desktop, and Windows tests were added during implementation.
- The gated Windows automation fixture passed with `SCREENSEARCH_RUN_AUTOMATION_IT=1`.

### Remaining boundary

P2/P3 gaps and release-hardening item 18 remain open. Guarded automation is included behind explicit default-off opt-in, not enabled by default.

## 2026-06-22 — Local generation throughput + chat-template fix

### Changed

- Diagnosed the "report generation does nothing" report from a live daemon log: a `Ministral-3-3B-Reasoning` GGUF loaded fully, then generation ran **CPU-only on a hardcoded two threads**, so a 3B reasoning model crawled and the answer pane appeared frozen.
- Replaced the hardcoded `n_threads(2)` / `n_threads_batch(2)` in `generate_gguf` with a physical-core budget (`cpu_thread_budget`, via `num_cpus`).
- Moved the 120 s generation deadline so it is armed **after** the model is resident and the prompt is decoded — a slow cold load can no longer silently consume the budget and return an empty "answered".
- Applied the model's own chat template (`apply_chat_format` → `LlamaModel::chat_template`/`apply_chat_template`) before tokenizing, so instruct and reasoning models receive the role markers + trailing assistant tag they expect instead of a raw text blob. Falls back to the raw prompt for base models with no template.
- Raised the evaluation context to `GENERATION_CONTEXT_TOKENS = 4096` and the per-request token cap to 768 so a reasoning model can finish its `<think>` span and still reach an answer (the wall-clock deadline stays the real bound). The daemon now stamps each model's `context_tokens` from that shared constant, keeping the stored metadata truthful.
- Added content-free generation diagnostics (`load_ms`, `prompt_tokens`, `prompt_decode_ms`, `generated_tokens`, `generate_ms`, `stop_reason`, `threads`) and broadened the worker's tracing filter so `screensearch_model_runtime` logs surface. No prompt/answer/query text is logged.

### Why

The model loaded fine; generation was just starved (two CPU threads) and unformatted (no chat template), so a reasoning model never surfaced an answer. These fixes are CPU-only and fully verifiable. GPU acceleration was explored but **descoped at the user's request** for this change; a later validation branch re-scoped GPU support as opt-in Vulkan worker acceleration with CPU fallback.

### Verification

- `cargo fmt --check` (exit 0), `cargo clippy --workspace --all-targets -- -D warnings` (exit 0), `cargo test --workspace` (52 passed, 7 ignored).

### Not changed

- The worker boundary contract, IPC, and persistence schema are unchanged. Items 15/16 remain open.

### PR #8 review follow-ups

- **Prompt-token budget** now derives from the constants (`GENERATION_CONTEXT_TOKENS - MAX_GENERATED_TOKENS = 3328`) instead of the stale hardcoded `1792` that assumed the old 2048-token context — a longer evidence prompt is no longer rejected for no reason.
- **Initial decode batch** is sized to the full context (`GENERATION_CONTEXT_TOKENS`) instead of a hardcoded `2048`; with the larger prompt budget a 2049–3328-token prompt would otherwise overflow a 2048-slot batch on the first decode.
- **Leading double-BOS guard** (`dedupe_leading_bos`): tokenizing the chat-formatted prompt with `AddBos::Always` keeps the tokenizer's single BOS for the built-in Mistral / Llama-2 / ChatML templates (which, in llama.cpp's C++ implementations, emit no literal BOS), while collapsing a `[BOS, BOS, …]` prefix if an exotic template (e.g. AlphaMonarch) emits a literal BOS that `parse_special` folds into a second one. The reviewer's suggested `AddBos::Never`-on-success was *not* taken: it would strip the only BOS from Mistral/Llama-2 prompts, which rely on the tokenizer to add it.
- **Chat-template fallback logging**: `apply_chat_format` now `warn!`s (content-free) when `LlamaChatMessage::new` or `apply_chat_template` fails before falling back to the raw prompt, so a silent templating failure is diagnosable. A missing template (base model) stays silent — that path is expected.
- Added `dedupe_leading_bos` unit tests (leading double collapses; single BOS, no-BOS, recurring-BOS, empty, and lone-token inputs are untouched).

### Updated verification

- `cargo fmt --check` (exit 0), `cargo clippy --workspace --all-targets -- -D warnings` (exit 0), `cargo test --workspace` (53 passed, 7 ignored), `npm run lint` (exit 0), `npm run build` (exit 0).

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

### Hardening (PR #4 automated review)

- **Hotkey persistence ordering** (`set_shell_settings`): register the new shortcut first and persist only on success, so a hotkey the OS rejects is never written to the settings file and cannot leave the next launch without a working summon shortcut.
- **Scoped shortcut replacement** (`apply_shortcut`): track the active shortcut in managed state and unregister only the previous binding (instead of `unregister_all`), restoring it if the new `register` call fails — a rejected hotkey no longer wipes the shortcut with no recovery.
- **Tray pause race** (`toggle_pause`): derive the target by atomically flipping a known-state `AtomicBool` (reconciled by the health poll) rather than reading-then-inverting, so two rapid clicks issue opposite requests instead of racing on a stale read; the optimistic flip rolls back if the request fails.
- **Summon-listener leak** (`App.tsx`): guard the `listen("summon-search")` effect with an `active` flag and dispose immediately if the component unmounts before the promise resolves, preventing a dangling Tauri event listener.

### Remaining boundary

Unchanged: patch-plan item 14 stays open pending the manual Windows tray/hotkey runtime check (spec §18). The pause/resume notification is best confirmed in a packaged build.

## 2026-06-21 — P3 model selection and worker boundary implementation

### Changed

- Created the `p3-model-selection-worker` branch.
- Added migration `0006_generation_model_catalog.sql` for selectable generation models with source kind, file metadata, hash, quantization, context, vision capability, and single-active selection.
- Extended domain and port contracts with generation-model catalog operations.
- Extended protobuf IPC additively with model-management requests, answer completion status fields, and worker-only OCR/embedding/generation messages.
- Added daemon model-management operations for local GGUF import, explicit Hugging Face download, active selection, inactive deletion, and unload.
- Added a real `screensearch-model-worker` named-pipe endpoint for Windows OCR, MiniLM embeddings, and llama.cpp generation.
- Changed daemon composition so production OCR, embedding, and generation ports call the model-worker pipe.
- Updated the desktop proxy, TypeScript API, Settings modal, and answer panel for model management and answer terminal states.

### Why

P3 needs model choice to be measured against the screen-memory use case rather than hard-coded. The branch supports local sample models, explicit Hugging Face downloads, and later bundled discovery while keeping evidence-first search usable without generation.

### Verification evidence

Not run in this implementation session. The Windows sandbox failed before launching commands, and the escalation reviewer rejected further command approval due to usage limits. Required follow-up commands remain `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `npm run lint`, and `npm run build`.

### Remaining boundary

Items 15 and 16 remain open until the branch compiles cleanly, the model-worker is verified against local GGUF candidates, and a benchmark report selects a default model for the use case. Legal/license approval remains intentionally deferred for this engineering slice.

## 2026-06-22 — P3 review-and-harden: worker supervision and model memory lifecycle

### Changed

- **Worker supervision (item 16).** Added `apps/daemon/src/supervisor.rs` with a sliding-window bounded `RestartPolicy` (exponential backoff, capped; failures outside the window decay) and a daemon `worker_supervisor_loop` that detects worker exits via `Child::wait`, restarts within budget, owns the clean-shutdown kill/reap, and surfaces budget exhaustion as a loud non-zero daemon exit. The worker is no longer spawned once and forgotten.
- **Orphan prevention (item 16).** Added a per-instance parent **lifeline pipe**: the daemon creates it (`create_worker_lifeline`) before spawning and passes `--lifeline-pipe`; the worker watches it (`watch_worker_lifeline`) and self-exits on EOF, so a daemon crash cannot leave an orphaned worker squatting the worker pipe. Pure safe Rust over the existing named-pipe transport — no new `unsafe`.
- **Memory lifecycle (item 15).** Added a daemon `idle_unload_loop` that, after an idle timeout, issues a **raw** worker unload (not the catalog-clearing daemon handler) so the active selection survives and the next query reloads lazily. Added a generation wall-clock deadline in `LlamaCppTextGenerator`, and made `is_loaded()` lock-free (an `AtomicBool`) so a health probe never blocks behind an in-flight generation holding the model mutex.
- **Revision integrity (ADR 0002).** `WorkerModelClient` now compares the worker-reported OCR/embedding revision against the expected id and fails loudly (`PortError::Internal` + content-free `warn!`) on drift instead of silently stamping derived records with a constant.
- **README.** Tightened the architecture line so the now-true "the daemon supervises it" claim describes crash detection, bounded restarts, and the lifeline.

### Tests

- New CI tests: `crates/persistence/tests/generation_model_catalog.rs` (catalog ordering, single-active invariant, delete-active denied, select-unregistered rejected, clear-active keeps rows), domain `GenerationModel::validate`/`ModelSourceKind::parse` cases, `crates/application/tests/answer_status.rs` (every terminal status branch via test doubles), `supervisor::tests` (restart-policy backoff/give-up/decay), and a model-runtime deadline-predicate test — 24 new CI-run tests.
- New gated `apps/daemon/tests/worker_supervision.rs` (5 `#[ignore]` cases, opt-in `SCREENSEARCH_RUN_WORKER_IT=1`; generation cases on `SCREENSEARCH_TEST_GGUF`): readiness + lifeline-exit, kill→restart recovery, generation round-trip after restart, cancellation keeps health responsive, and the idle-unload primitive.

### Decisions made

- Tunables (restart budget/backoff, idle timeout, generation deadline) are **code constants**, not environment variables (operating rule 5; the spec names none).
- Chose a parent-lifeline pipe over a Windows Job Object to keep the workspace free of `unsafe`.
- Memory-pressure-triggered unload is deferred (idle-timeout unload ships); the pressure-signal source is recorded as **GAP-008** so item 15's "memory lifecycle" is honestly scoped.
- The `answer_status` test uses an inline fake `ArchiveRepository` (test double) rather than adding `application → persistence` as a dev-dependency.

### Verification evidence

- `cargo fmt --check` — clean (exit 0); the only earlier breakage was rustfmt/CRLF drift left by the prior session, fixed by `cargo fmt`.
- `cargo clippy --workspace --all-targets -- -D warnings` — clean (exit 0).
- `cargo test --workspace` — **52 passed, 0 failed, 7 ignored** (the 5 new gated worker-supervision cases plus the pre-existing scale and live-archive tests). Includes the new catalog (7), answer-status (7), domain generation-model (5), supervisor (4), and deadline (1) tests.
- `npm run lint` and `npm run build` (`apps/desktop`) — clean (exit 0).

### Remaining boundary

Items 15 and 16 **stay open**. Closing them additionally requires a live-Windows GGUF benchmark of a selected candidate model (the harness exists; candidate measurements are pending), the GAP-002/GAP-003 model/licensing decisions, and the memory-pressure unload path (GAP-008). The gated worker-supervision tests must also be run on Windows with a test GGUF to confirm the runtime end to end.

## 2026-06-22 — PR #6 review follow-ups (model-management polish)

### Changed

- **Deleting a model now removes its directory too.** `delete_generation_model` removed the GGUF file but left the empty `{model_root}/{model_id}/` directory behind to accumulate. It now best-effort-removes the parent directory after the file delete (ignoring `NotFound`; a still-populated or busy directory is logged content-free and left in place rather than failing the delete).
- **Hugging Face downloads reject HTML/error pages.** `download_and_hash` now checks the response `Content-Type` and bails with a clear message when the server returns a `text/*` page (e.g. a gated-model authentication wall) instead of writing a bogus file that would only fail later with an opaque "invalid GGUF magic" error. `error_for_status()` already rejected explicit 401/403 responses; this closes the 200-with-HTML case.

### Not changed (reviewed and intentionally left)

- **`context_tokens = 2048`** is correct as stored: the domain field is documented as "context window *configured for evaluation*", and the worker's llama.cpp context is a fixed conservative `n_ctx = 2048` with a `1792`-token prompt cap. The stored value matches the effective runtime context, so it is truthful rather than a fabricated capability. Reading the model's GGUF-declared `context_length` and raising the evaluation window is tracked as a follow-up (see patch plan).
- **Import/download progress feedback** and **a Hugging Face `revision` pin / gated-model token auth** are deferred: both require additive IPC contract plus Tauri/UI plumbing (progress) or a wire/UI argument (revision/token) and are larger than this review-fix pass. Recorded in the patch plan; revision/auth ties to GAP-003. Provenance is already captured today via the stored BLAKE3 `content_hash`.

### Verification evidence

- `cargo fmt --check` — clean (exit 0).
- `cargo clippy --workspace --all-targets -- -D warnings` — clean (exit 0).
- `cargo test --workspace` — 52 passed, 0 failed, 7 ignored (unchanged; the two fixes are in daemon binary helpers that touch the filesystem/network and are not unit-testable in CI).
- `npm run lint` and `npm run build` (`apps/desktop`) — clean (exit 0); no desktop code changed.
