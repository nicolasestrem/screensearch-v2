# ScreenSearch V2 Strategic Plan

Last updated: 2026-06-21

This document answers one question: **what should change, and why?** Detailed contracts belong in `03_MASTER_PRODUCTION_SPEC.md`.

## Goal

Turn the verified bootstrap plumbing into a trustworthy local screen-memory product. The differentiator is not an LLM chat surface; it is fast, private retrieval with visual proof.

## User value

The user should not need to remember exact wording, manually create captures, manage queues, or trust uncited prose. ScreenSearch should quietly build a searchable local memory and make every result inspectable.

## Future-state architecture

Retain the existing modular monolith and isolated model-worker boundary:

1. A persistent daemon captures eligible Windows desktop frames, applies privacy and deduplication policy, stores assets, and creates durable analysis jobs.
2. A supervised model worker loads one heavy model class at a time and serves OCR, embedding, and generation through a versioned local contract.
3. libSQL owns metadata, FTS5, vectors, jobs, and outbox events; the filesystem owns immutable image assets and model files.
4. A small Tauri shell owns tray behavior, global hotkeys, window lifecycle, and presentation. It never owns durable indexing state.
5. Automation remains a separate, deny-by-default adapter reached only through explicit approval and safety gates.

This keeps deployment simple for one desktop user while preserving fault and memory isolation around native AI runtimes. Do not split ordinary domain capabilities into network microservices.

## Priorities

### P0 — Truthful evidence loop

- Capture real Windows pixels.
- Encode stable image assets.
- Automatically drain durable jobs.
- Run real local OCR.
- Return and render evidence metadata and positioned OCR matches.
- Prove exact-text search latency and restart recovery.

Why: until this works, neither semantic search nor visual design can be judged honestly.

### P1 — Semantic retrieval and scale

- Add a compact local embedding model behind the existing port.
- Preserve model revision consistency.
- Fuse FTS5 and vector results.
- Add change-aware deduplication, retention, disk budgets, and indexing metrics.
- Exercise 10 million synthetic metadata rows without requiring 10 million image assets.

Why: imperfect memory is the core search problem, but semantic ranking must build on verifiable evidence.

### P2 — Product shell

- Replace manual pipeline controls with tray status and pause/resume.
- Add a configurable global hotkey.
- Use one compact search window with keyboard navigation.
- Show screenshot-first results, metadata, highlighted regions, and a detail/timeline view.
- Add privacy exclusions, retention, storage, and model settings.

Why: the application should feel like a native retrieval utility, not an operations dashboard.

### P3 — Constrained synthesis

- Add a local quantized generator in the isolated worker.
- Generate only after retrieval, cite capture identifiers, and expose the supporting images.
- Return a clear no-evidence state rather than filling gaps.
- Keep model loading lazy and unload idle generation models under memory pressure.

Why: synthesis is useful only after users can inspect and trust retrieval.

### P4 — Guarded automation

- Keep automation disabled by default.
- Represent plans as deterministic typed actions.
- Require per-plan approval, foreground-window identity, timeout, rate limits, and a global emergency abort hotkey.
- Add Windows UI Automation where possible and `SendInput` only as a controlled fallback.

Why: search and recall are low-risk; input emission changes external state and needs a separate trust boundary.

## Delivery phases

1. Specifications and observability baseline.
2. Native capture and automatic worker loop.
3. OCR and evidence-rich lexical search.
4. Embeddings and hybrid ranking.
5. Three product UI directions, user selection, then implementation.
6. Local cited generation.
7. Guarded automation adapters and Windows validation.
8. Reliability, scale, packaging, and release hardening.

## Migration path

There is no V1 migration. Within V2, every schema change is forward-only and migration-backed. Derived OCR and embedding data may be rebuilt from immutable assets when a model revision changes. Original capture metadata and assets are never silently rewritten.

## Risks and mitigations

- **Capture overhead:** use change detection, bounded cadence, backpressure, and measurable CPU/GPU budgets.
- **Sensitive content:** exclusions, pause, private-window policy, local-only ACLs, deletion, and no content logging.
- **Model memory pressure:** isolate inference, lazy-load, serialize heavy work, use quantized models, and unload on idle.
- **OCR quality:** keep positioned evidence, language metadata, confidence, and re-indexability.
- **Vector scale:** benchmark the actual libSQL index, filter by model revision, and retain lexical fallback.
- **Hallucinated answers:** require retrieved evidence and expose citations before answer tokens.
- **Automation harm:** deny by default and maintain approval, focus, timeout, and abort invariants.
- **UI churn:** defer production UI until realistic result data exists and select a visual target before implementation.

## Product measures

- Exact visible phrase returns the correct capture in less than 1 second at p95 on the reference machine.
- Indexing catches up after daemon restart without user action or duplicate derived rows.
- Idle capture/index CPU and memory remain within budgets defined after baseline measurement.
- Every displayed answer sentence is traceable to at least one visible citation.
- A user can pause capture and understand current capture state at a glance.
