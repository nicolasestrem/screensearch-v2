# ScreenSearch V2 Patch Plan

Last updated: 2026-06-21

Items are ordered. An agent may continue independent work around a blocked item but must not silently reorder product dependencies.

## Active

1. [x] Extend capture/domain contracts with an explicit image media type and evidence metadata.
2. [x] Implement and test a real Windows image capture adapter with foreground process/title metadata.
3. [x] Compose the real adapter in production; keep fake capture test-only.
4. [x] Add automatic capture scheduling and continuous durable analysis with graceful shutdown.
5. [x] Implement real offline Windows OCR returning positioned text and a stable provider revision.
6. [x] Extend persistence and IPC with screenshot evidence, asset authorization, metadata, provenance, and OCR bounds.
7. [x] Add evidence-only search and render real screenshots/results in the diagnostics surface.
8. [x] Add quantized MiniLM ONNX embeddings and activate its 384-dimensional revision.
9. [ ] Add queue high/low-water backpressure, perceptual deduplication, and capture pause/exclusion policy.
10. [ ] Benchmark hybrid ranking, exact-match boost, model isolation, latency, CPU, memory, and disk growth.
11. [ ] Implement exclusions, queue status, retention, deletion, and disk-budget policies; pause/resume is complete.
12. [x] Generate exactly three grounded visual directions and obtain product-owner selection of Memory Timeline.
13. [x] Record the selected visual target and binding decisions in `docs/design`.
14. [ ] Add tray lifecycle, a system-wide hotkey, and complete keyboard navigation; the selected search/timeline/evidence/settings interface is implemented and has passed visual QA.
15. [ ] Select/install a GGUF model and validate the implemented llama.cpp provider, citations, cancellation, and memory lifecycle.
16. [ ] Move OCR, embedding, and generation execution behind the supervised model-worker boundary.
17. [ ] Implement typed Windows automation emission behind existing approval/focus/abort gates, or keep the feature disabled if safety requirements cannot be proven.
18. [ ] Run recovery, crash, saturation, restart, 10-million-row, Windows end-to-end, security, packaging, and visual QA verification.
19. [ ] Replace this plan with only genuine remaining gaps and prepare a release review.

## Definition for closing an item

An item is closed only when its production path is composed, tests cover success and failure behavior, user-visible claims are truthful, and the build review/changelog are updated. A placeholder, fake, unreachable adapter, or compile-only implementation does not close an item.
