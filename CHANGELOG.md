# Changelog

## Unreleased

### Added

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

### Changed

- Addressed PR #12 review follow-ups by capping per-hit OCR prompt excerpts, clarifying local-day planning, matching source filters against OCR text for browser pages, allowing HF downloads without a display-name field, and making `<think>` stripping robust for unclosed streaming spans.
- Applied search time/source filters in backend hybrid retrieval before ranking, loosened FTS to phrase/exact boosts plus OR fallback, and enriched answer prompts with local timestamp/source metadata and citation/uncertainty requirements.
- Redesigned Settings around answer readiness, timezone basis, active/installed answer models, blank guided local GGUF import, advanced HF download fields, explicit storage policy state, and conservative reset.
- Hid generated model `<think>` spans before rendering answers in the desktop UI.
- Hardened llama.cpp Vulkan sidecar installs with serialized acquisition, bounded network and extraction work, prerelease filtering, and safe sidecar execution before CPU fallback.
- Fixed llama.cpp sidecar stdout decoding so generated answers stream visible tokens, and made the evidence rail buttons select/focus recent or visual evidence instead of becoming no-ops.
- Incrementally sanitized llama.cpp sidecar stdout before displaying answers, added content-free sidecar lifecycle logs, disabled sidecar reasoning output where supported, pinned deterministic sampler flags, capped sidecar downloads before extraction, and made release overrides reinstall when the installed sidecar tag differs.

### Documentation

- Recorded known gaps for source-vocabulary expansion, client timezone propagation, and further prompt hardening for application/window-title metadata.
- Added `docs/design/useful-local-answers.md` and narrowed the remaining patch plan to genuine item-15 and item-18 release gaps.
- Addressed PR #10 review feedback by making the GGUF full-GPU-offload sentinel explicit and correcting stale Claude changelog guidance.
- Recorded local P3 GGUF smoke measurements and the remaining item-15 release blockers in `docs/performance/P3_MODEL_SELECTION.md`.
- Documented the sidecar-first GPU path and moved the Vulkan SDK Cargo build to advanced/local-build guidance.
- Added `docs/design/p4-guarded-automation.md` and updated the repository specifications, known gaps, build review, and AI-assisted changelog to record the delivered P4 state.
