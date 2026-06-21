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
