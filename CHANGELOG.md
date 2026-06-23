# Changelog

## Unreleased

### Documentation

- Made `AGENTS.md` a thin pointer to `CLAUDE.md` (the single source of truth) instead of a parallel summary, so the two guides cannot drift.
- Corrected `CLAUDE.md` drift against the current code and ADRs: the `model-worker` is now the supervised owner of OCR/embedding/generation (not "reserved / no inference"), the daemon delegates inference via `WorkerModelClient` while wiring only capture and automation in-process, the `ports` automation traits are `AutomationRepository`/`AutomationPlatform` (not `AutomationExecutor`), and the automation invariant now matches ADR 0004 (model output cannot create/approve/execute a plan; five fail-closed pre-action checks). Added build prerequisites (Rust 1.88, Node 22, WebView2), the required `cargo build -p screensearch-model-worker` step, and pointers to the `ipc::convert` module, the desktop hooks, and `docs/performance/`/`design-qa.md`/`AGENTS.md`.

### Added

- Closed P0/P1 spec-deviation gaps found in a scrupulous Phase 0→1 review: persisted the full embedding manifest (provider, model name, revision/hash, tokenizer revision, pooling, normalization, license, source URL) via migration `0008` and a matching `FastEmbedEngine::manifest()` accessor (spec §8.1).
- Added a grace-period filesystem orphan sweep that removes asset files left by a capture whose database commit failed, never deleting referenced or recently written files (spec §5), wired into the daemon maintenance loop.
- Added the §18 integration tests that were missing: expired-lease recovery, end-to-end queue saturation/backpressure resume, and capture-commit orphan handling.
- Added deterministic local-time search planning for useful local answers, including source/time filters for Telegram, GitHub PR, Codex settings, and Amazon book prompts.
- Added a streamed search-plan event so the desktop can show interpreted retrieval terms, time bounds, source hints, and timezone basis before citations.
- Added a content-free opt-in local archive answer smoke covering the four supplied prompts.
- Closed the P2 product-shell item after user-attested native Windows tray/hotkey verification.
- Validated the supervised model-worker path against two local GGUF candidates and closed patch-plan item 16.
- Added opt-in Vulkan GPU offload for GGUF generation in the model worker, with runtime detection and CPU fallback.
- Added a Windows llama.cpp Vulkan sidecar acquisition path for GGUF generation, with safe zip install, incomplete-release skipping, release URL pinning, and embedded CPU fallback.
- Implemented Guarded Automation V1 behind explicit default-off opt-in.
- Added domain validation, content-free persistence, daemon orchestration, typed IPC/Tauri commands, native Windows UIA/keyboard emission, and the manual approval UI.
- Added a gated synthetic Windows automation fixture for opt-in native verification.
- Unloaded the resident generation model on a Windows low-memory resource notification in addition to the idle timeout (GAP-008, spec §11): a safe `MemoryPressureMonitor` wraps `CreateMemoryResourceNotification`, the existing idle-unload loop queries it each tick, and a pure `should_unload_generation` policy is unit-tested. The live pressure path is pending manual Windows attestation.

### Changed

- Excluded the volatile window `display_title` from the canonical automation plan digest (and lowercased the executable name to match the case-insensitive identity comparison), so a window that retitles itself between approve and execute no longer causes a spurious `plan_mismatch`. Approval now binds only to target identity (PID/HWND/executable) plus the ordered actions, consistent with `AutomationTarget::matches_identity`; the title remains review-only context and is still never persisted. Key-chord modifiers are normalized to an unordered set in the digest, so listing the same modifiers in a different order at execute time cannot cause a mismatch.
- Added `heartbeat_fresh` and `abort_registered` to the guarded-automation status so the desktop renders a three-state abort-shortcut pill — "Live", "Unavailable" (hotkey not held; choose another), or "Reconnecting" (daemon link lagging) — instead of one ambiguous "Unavailable".
- Centralized guarded-automation key/modifier conversion in a shared `screensearch-ipc::convert` module used by both the daemon (wire → domain) and the desktop shell (UI token → wire). The accepted UI token vocabulary is single-sourced in the domain (`AutomationKey::ui_token`/`KeyModifier::ui_token`) and the domain↔wire mappings are exhaustive, so adding a key variant fails to compile until it is handled in every direction rather than silently becoming unproducible from the UI.
- Hardened answer prompts against adversarial capture metadata (GAP-011): the untrusted application, window title, and OCR excerpt are sanitized before entering the prompt — control characters and newlines collapse to single spaces, `[`/`]` are rewritten to their fullwidth forms, and a zero-width space is inserted after every `<` to break chat-template/special-token delimiters (`<|im_start|>`, `</s>`) since the llama.cpp path tokenizes with special-token parsing enabled — so a hostile window title can neither begin its own prompt line, forge a `[capture-id]` citation, nor inject chat-template control tokens; covered by `prompt_neutralizes_adversarial_metadata` and `sanitize_untrusted_field_breaks_chat_template_delimiters`.
- Corrected a stale patch-plan note: the fixed generation evaluation context is `GENERATION_CONTEXT_TOKENS` (4096), not 2048.
- Hardened the orphan asset sweep after PR review: candidate-first walk that skips symlinked entries (root-confinement) and tolerates files vanishing mid-walk, bounded `IN (...)` reconciliation instead of loading every asset hash, an mtime refresh on reused files to close a capture/sweep race, and once-per-hour throttling.
- Clarified embedding-manifest provenance: `revision_hash` is the advertised upstream revision (fastembed downloads `main` unpinned); within-archive isolation is enforced by `model_id`, with hard pinning tracked under GAP-002/GAP-003.
- Added deterministic "equal jitter" to the analysis-job retry backoff (`base/2 + hash(job, attempt) mod base/2`) so retries de-correlate while staying reproducible and bounded by the exponential cap (spec §6).
- Normalized Windows OCR block text to LF line endings and Unicode NFC before persistence so stored text, FTS terms, and embeddings share one canonical form (spec §7.2).
- Refactored the libSQL migration runner to a single versioned-gate helper while adding migration `0008`.
- Addressed PR #12 review follow-ups by capping per-hit OCR prompt excerpts, prompting every returned citation, clarifying local-day planning, matching source filters at capture level for browser pages, preserving Unicode query terms, avoiding unsupported day-modifier time filters, allowing HF downloads without a display-name field, and making `<think>` stripping robust for unclosed streaming spans.
- Applied search time/source filters in backend hybrid retrieval before ranking, loosened FTS to phrase/exact boosts plus OR fallback, and enriched answer prompts with local timestamp/source metadata and citation/uncertainty requirements.
- Redesigned Settings around answer readiness, timezone basis, active/installed answer models, blank guided local GGUF import, advanced HF download fields, explicit storage policy state, and conservative reset.
- Hid generated model `<think>` spans before rendering answers in the desktop UI.
- Hardened llama.cpp Vulkan sidecar installs with serialized acquisition, bounded network and extraction work, prerelease filtering, and safe sidecar execution before CPU fallback.
- Fixed llama.cpp sidecar stdout decoding so generated answers stream visible tokens, and made the evidence rail buttons select/focus recent or visual evidence instead of becoming no-ops.
- Incrementally sanitized llama.cpp sidecar stdout before displaying answers, added content-free sidecar lifecycle logs, disabled sidecar reasoning output where supported, pinned deterministic sampler flags, capped sidecar downloads before extraction, and made release overrides reinstall when the installed sidecar tag differs.

### Fixed

- Fixed guarded-automation approval always failing through the UI with `target_changed`: the `approve_automation` Tauri command now hides the ScreenSearch window before contacting the daemon (exactly as capture and execute already did), so the captured target — not ScreenSearch — is the foreground window during the daemon's approval-time foreground-identity check. The hide → settle → restore choreography is now a single shared `with_foreground_yielded` helper used by capture, approve, and execute, with the window restored via a `Drop` guard so a cancelled or panicking command can never leave it hidden. Escaped because the service tests used a fake platform that always returned a matching target and the gated native fixture bypassed approval.
- Closed a guarded-automation fail-open in the abort shortcut: the shell now re-asserts the fixed `Ctrl+Alt+Shift+Esc` registration at the OS level on every 3-second heartbeat (`unregister` then `register`, whose result reflects the real `RegisterHotKey`) instead of trusting a single startup snapshot or the plugin's cache-only `is_registered`, so a hotkey the OS silently reclaimed can no longer be reported to the daemon as live. The user is notified once when the abort shortcut becomes unavailable.
- Fixed OCR highlight overlays drifting off the screenshot text whenever a capture's aspect ratio differed from the inspector box (ultrawide, portrait/rotated monitors, or when the `max-height` clamp engaged): overlays are now measured against the actually-rendered `object-fit: contain` image rectangle (via a `ResizeObserver`) instead of the letterboxed container, in both the timeline thumbnail and the large detail view.
- Fixed the guarded-automation dialog not trapping `Tab`/`Shift+Tab` focus (the settings dialog already did), and stopped `Home`/`End` from also moving the evidence timeline while a detail tab is focused; the dialog focus trap is now a single shared hook used by both dialogs.

- Replaced the desktop preview/QA capture asset with a committed, self-contained synthetic SVG fixture so browser QA exercises overlay alignment on any clone without depending on a gitignored real screen capture, and hardened the dialog focus trap (filters non-focusable/hidden/non-rendered elements, traps `<summary>`, recovers escaped focus).

### Documentation

- Recorded the P2 OCR-overlay alignment defect (missed by the original visual QA, which exercised synthetic bounds over blank space) and its fix in `docs/design/p2-shell.md`, including a new non-16:9 / portrait overlay-alignment check in the manual Windows runbook.
- Recorded known gaps for source-vocabulary expansion, client timezone propagation, and further prompt hardening for application/window-title metadata.
- Added `docs/design/useful-local-answers.md` and narrowed the remaining patch plan to genuine item-15 and item-18 release gaps.
- Addressed PR #10 review feedback by making the GGUF full-GPU-offload sentinel explicit and correcting stale Claude changelog guidance.
- Recorded local P3 GGUF smoke measurements and the remaining item-15 release blockers in `docs/performance/P3_MODEL_SELECTION.md`.
- Documented the sidecar-first GPU path and moved the Vulkan SDK Cargo build to advanced/local-build guidance.
- Added `docs/design/p4-guarded-automation.md` and updated the repository specifications, known gaps, build review, and AI-assisted changelog to record the delivered P4 state.
- Recorded the P4 guarded-automation post-closure review (one feature-breaking approval defect plus abort-registration, digest, and key-mapping hardening) in `specs/06_PATCH_PLAN.md` and `docs/design/p4-guarded-automation.md`, documented the between-actions abort/timeout granularity and the STA UIA-client note in `crates/windows/src/automation.rs`, and added a manual end-to-end approve→execute attestation step to the P4 verification runbook.
