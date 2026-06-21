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
9. [x] Add queue high/low-water backpressure, conservative perceptual deduplication, single-flight capture, and pre-persistence pause/self-exclusion policy.
10. [x] Benchmark hybrid ranking, exact-match boost, model-revision isolation, latency, CPU, memory, database size, and ten-million-row metadata growth on the named P1 reference machine.
11. [x] Implement persisted user-configurable exclusions, age retention, captured-asset disk budgets, storage metrics, transactional capture deletion, and durable unreferenced-asset cleanup.
12. [x] Generate exactly three grounded visual directions and obtain product-owner selection of Memory Timeline.
13. [x] Record the selected visual target and binding decisions in `docs/design`.
14. [ ] Add tray lifecycle, a system-wide hotkey, and complete keyboard navigation; the selected search/timeline/evidence/settings interface is implemented and has passed visual QA.
15. [ ] Select/install a GGUF model and validate the implemented llama.cpp provider, citations, cancellation, and memory lifecycle.
16. [ ] Move OCR, embedding, and generation execution behind the supervised model-worker boundary.
17. [ ] Implement typed Windows automation emission behind existing approval/focus/abort gates, or keep the feature disabled if safety requirements cannot be proven.
18. [ ] Complete locked-session privacy handling plus remaining recovery, worker-crash, Windows end-to-end, security, packaging, and release-hardware soak verification; saturation, 10-million-row metadata scale, live evidence latency, and settings visual QA are complete.
19. [ ] Replace this plan with only genuine remaining gaps and prepare a release review.

## Notes

- 2026-06-21 P0/P1 review: items 1–13 remain closed. A review pass added explicit analysis failure-path tests (retry backoff, dead-letter promotion at `MAX_JOB_ATTEMPTS`, embedding-dimension rejection at the persistence and `process_one` layers) so items 4 and 8 now meet the success-and-failure test bar below. No item was reopened; no production code changed.
- 2026-06-21 P2 shell pass (item 14, partial): implemented and composed in production the system tray (live capture-state tooltip + status line, Pause/Resume, Open, Quit), a configurable system-wide summon hotkey (default `Ctrl+Shift+Space`, registered from Rust via `tauri-plugin-global-shortcut`, persisted as a shell-local JSON setting), hide-to-tray on window close, and complete keyboard navigation (timeline arrow/Home/End with roving tab index, ARIA tablist arrow keys, modal focus trap, Escape-to-close with focus restore, Ctrl+K search focus). Verified: `cargo fmt`/`clippy -D warnings`/`test --workspace`, `npm run lint`/`build`, and browser-driven keyboard-navigation QA against the dev server (arrow/Home/End selection + focus, typing guard, tablist arrows, focus trap, Escape restore, hotkey capture). **Item 14 stays open** until the native tray, global hotkey, and hide-to-tray runtime are confirmed in a manual Windows `npm run tauri dev` session against a running daemon (the spec §18 "Verify tray pause/resume and global hotkey" check); `cargo test` only compiles the shell, it does not exercise the GUI.

## Definition for closing an item

An item is closed only when its production path is composed, tests cover success and failure behavior, user-visible claims are truthful, and the build review/changelog are updated. A placeholder, fake, unreachable adapter, or compile-only implementation does not close an item.
