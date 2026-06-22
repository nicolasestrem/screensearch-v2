# Changelog

## Unreleased

### Added

- Closed the P2 product-shell item after user-attested native Windows tray/hotkey verification.
- Validated the supervised model-worker path against two local GGUF candidates and closed patch-plan item 16.
- Added opt-in Vulkan GPU offload for GGUF generation in the model worker, with runtime detection and CPU fallback.
- Implemented Guarded Automation V1 behind explicit default-off opt-in.
- Added domain validation, content-free persistence, daemon orchestration, typed IPC/Tauri commands, native Windows UIA/keyboard emission, and the manual approval UI.
- Added a gated synthetic Windows automation fixture for opt-in native verification.

### Documentation

- Addressed PR #10 review feedback by making the GGUF full-GPU-offload sentinel explicit and correcting stale Claude changelog guidance.
- Recorded local P3 GGUF smoke measurements and the remaining item-15 release blockers in `docs/performance/P3_MODEL_SELECTION.md`.
- Documented the GPU build path and the local Vulkan SDK blocker.
- Added `docs/design/p4-guarded-automation.md` and updated the repository specifications, known gaps, build review, and AI-assisted changelog to record the delivered P4 state.
