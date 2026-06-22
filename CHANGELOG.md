# Changelog

## Unreleased

### Added

- Closed the P2 product-shell item after user-attested native Windows tray/hotkey verification.
- Validated the supervised model-worker path against two local GGUF candidates and closed patch-plan item 16.
- Added opt-in Vulkan GPU offload for GGUF generation in the model worker, with runtime detection and CPU fallback.
- Added a Windows llama.cpp Vulkan sidecar acquisition path for GGUF generation, with safe zip install, incomplete-release skipping, release URL pinning, and embedded CPU fallback.
- Implemented Guarded Automation V1 behind explicit default-off opt-in.
- Added domain validation, content-free persistence, daemon orchestration, typed IPC/Tauri commands, native Windows UIA/keyboard emission, and the manual approval UI.
- Added a gated synthetic Windows automation fixture for opt-in native verification.

### Changed

- Hardened llama.cpp Vulkan sidecar installs with serialized acquisition, bounded network and extraction work, prerelease filtering, and safe sidecar execution before CPU fallback.
- Fixed llama.cpp sidecar stdout decoding so generated answers stream visible tokens, and made the evidence rail buttons select/focus recent or visual evidence instead of becoming no-ops.
- Incrementally sanitized llama.cpp sidecar stdout before displaying answers, added content-free sidecar lifecycle logs, disabled sidecar reasoning output where supported, pinned deterministic sampler flags, capped sidecar downloads before extraction, and made release overrides reinstall when the installed sidecar tag differs.

### Documentation

- Addressed PR #10 review feedback by making the GGUF full-GPU-offload sentinel explicit and correcting stale Claude changelog guidance.
- Recorded local P3 GGUF smoke measurements and the remaining item-15 release blockers in `docs/performance/P3_MODEL_SELECTION.md`.
- Documented the sidecar-first GPU path and moved the Vulkan SDK Cargo build to advanced/local-build guidance.
- Added `docs/design/p4-guarded-automation.md` and updated the repository specifications, known gaps, build review, and AI-assisted changelog to record the delivered P4 state.
