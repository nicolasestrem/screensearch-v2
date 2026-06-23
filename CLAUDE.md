# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

ScreenSearch V2 is an offline-first, single-user **Windows** desktop app: it captures screen changes, OCRs them locally, indexes lexical + vector representations, and returns citation-backed (screenshot-grounded) answers. It is a clean-room rewrite — there is no V1 code, shared history, or migration path (ADR 0005). The launch target is Windows-only and fully offline, scaling to ~10M captures; macOS/Linux remain explicit-but-unimplemented ports.

## Commands

**Prerequisites:** Rust 1.88+ (the workspace pins `rust-version = "1.88"` but there is **no** `rust-toolchain.toml`, so select the toolchain yourself), Node.js 22+, npm, and the Windows WebView2 runtime. Optionally set `SSV2C_LLAMA_RELEASE_URL` to pin the llama.cpp Vulkan sidecar release.

Rust (run from repo root):

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings   # warnings are errors in CI
cargo test --workspace
cargo test -p screensearch-application <test_name>       # single test by name
cargo run -p screensearch-daemon                         # run the daemon (see env below)
```

Desktop (run from `apps/desktop`):

```powershell
npm ci
npm run lint        # eslint
npm run build       # tsc -b && vite build  (also the typecheck gate)
npm run dev         # vite dev server (browser preview mode, see below)
npm run tauri dev   # full Tauri shell against a running daemon
```

CI (`.github/workflows/ci.yml`) runs the three Rust checks and the three desktop checks on `windows-latest`. Both must pass.

### Running the daemon locally

Point the daemon at an isolated data dir so you never touch real user data:

```powershell
$env:SCREENSEARCH_DATA_DIR = "$PWD\.local-data"
cargo build -p screensearch-model-worker   # daemon does NOT auto-build sibling binaries
cargo run -p screensearch-daemon
```

The daemon spawns the model worker from beside its own executable, so when running from source you must build `screensearch-model-worker` first (packaged builds place it next to the daemon). Without `SCREENSEARCH_DATA_DIR` it uses `%LOCALAPPDATA%\ScreenSearchV2`. The daemon listens on the named pipe `\\.\pipe\screensearch-v2`; the desktop shell connects to it, and the daemon connects to the worker over a second pipe.

### Gated / ignored tests

These are `#[ignore]`d and require explicit opt-in:

- `crates/persistence/tests/synthetic_scale.rs` — set `SCREENSEARCH_RUN_SCALE_BENCH=1` (optionally `SCREENSEARCH_SCALE_ROWS`). Long-running 10M-row benchmark.
- `crates/persistence/tests/live_windows_archive.rs` — needs a populated `SCREENSEARCH_DATA_DIR` and local model cache; optional `SCREENSEARCH_SMOKE_QUERY`.
- `apps/daemon/tests/worker_supervision.rs` — set `SCREENSEARCH_RUN_WORKER_IT=1` and first build the worker (`cargo build -p screensearch-model-worker`). Spawns the real model worker to exercise readiness, the parent lifeline, kill→restart recovery, cancellation, and idle unload. Generation cases also need `SCREENSEARCH_TEST_GGUF` (a path to a local `.gguf`) and skip cleanly if it is unset.

## Architecture

This is a Rust 2024 Cargo workspace plus a Tauri 2 / React 19 / TypeScript desktop app. It follows **ports-and-adapters (hexagonal)** layering with a strict inward dependency rule: adapters depend on `ports` and `domain`; **`domain` imports nothing from persistence, IPC, UI, model runtimes, or Windows APIs.** Respect this when adding code.

### Crates (`crates/`)

- `domain` — pure types and invariants (`CaptureId`, `SearchHit`, `ArchiveSettings`, etc.). IDs are UUIDv7 (time-sortable). No I/O, no adapter imports.
- `ports` — the dependency-inversion traits: `CaptureSource`, `AssetStore`, `ArchiveRepository`, `OcrEngine`, `EmbeddingEngine`, `TextGenerator`, `AutomationRepository`, `AutomationPlatform`, and the shared `PortError`.
- `application` — use-case orchestration over ports, owning no adapter. Three services: `IngestService` (capture → fingerprint → store → transactional enqueue), `AnalysisService` (claim durable job → OCR → embed → commit atomically), `SearchService` (embed query → hybrid retrieve → stream citations then answer tokens). Also `CapturePolicy` (pause, queue backpressure hysteresis, app/title exclusions, perceptual near-duplicate filter).
- `persistence` — `LibSqlArchive` (embedded libSQL, WAL) + `FileAssetStore` (content-addressed files, write-temp + atomic rename). Owns SQL migrations and `hybrid_search` (FTS5 lexical + brute-force vector, fused and re-ranked).
- `ipc` — versioned Protobuf contract + Windows named-pipe transport (length-delimited frames). Rust types are **generated at build time** from the `.proto` (see below). `convert.rs` is the single source of truth for UI ↔ wire ↔ domain key/modifier mappings used by automation (round-trip tested) — don't duplicate those conversions elsewhere.
- `model-runtime` — real local providers, wired **inside the `model-worker`** (not the daemon): `FastEmbedEngine` (quantized all-MiniLM-L6-v2, 384-dim, via `fastembed`) for embeddings, and for generation either `LlamaSidecarTextGenerator` (preferred GPU path — a prebuilt llama.cpp Windows Vulkan sidecar auto-installed under the data dir, see `llama_sidecar.rs`) or the embedded `LlamaCppTextGenerator` (CPU, GGUF via `llama-cpp-2`) as fallback. Both are real — generation never falls back to a fake. Also the **test-only** `Fake*` providers. The Windows OCR adapter lives in the `windows` crate, not here.
- `windows` — Windows-facing adapters: `WindowsGraphicsCaptureSource` (xcap), `WindowsOcrEngine` (Windows Media OCR), and automation.

### Apps (`apps/`)

- `daemon` — the persistent process and **where production adapters are wired** (`apps/daemon/src/main.rs`). It wires the capture source (`WindowsGraphicsCaptureSource`) and automation platform (`WindowsAutomationPlatform`) **in-process**, but delegates OCR, embedding, and generation to the supervised `model-worker` via `WorkerModelClient` (which implements `OcrEngine`, `EmbeddingEngine`, and `TextGenerator` over a second named pipe). Also owns the libSQL repo, the IPC handler (`DaemonHandler`), worker supervision (crash detection + bounded restart + parent lifeline), and background loops (capture cadence, analysis pump, retention/maintenance, and idle/low-memory generation-model unload).
- `desktop/src-tauri` — Tauri shell. Each `#[tauri::command]` is a thin typed proxy that opens an `IpcClient`, sends one Protobuf request, and maps the response to a camelCase JSON struct for the UI. It holds no business logic.
- `desktop/src` — React UI. The selected product direction is **Memory Timeline** (see `docs/design/README.md`): search bar on top, chronologically grouped screenshot evidence on the left, the selected screenshot as the dominant artifact with OCR-bound overlays, subordinate metadata/provenance, and an optional cited answer. `api.ts` has an `isTauri` check that serves **fake preview data in a plain browser** (`npm run dev`) and real IPC calls inside the Tauri shell. State is plain React + React Query (async/health polling); note the custom hooks `useContainedRect` (positions OCR overlays over an `object-fit: contain` image) and `useDialogFocusTrap` (modal focus trap) in `App.tsx`.
- `model-worker` — a **supervised** child process that owns the native inference runtimes: OCR, embedding, and GGUF generation (ADR 0001). The daemon spawns it, ties its lifetime to the daemon (parent lifeline), and restarts it within a bounded budget on crash without losing archive state. Model weights are referenced by path, never copied inline through IPC.

### Data & control flow

UI → Tauri command → Protobuf `RequestEnvelope` over named pipe → `DaemonHandler` → `application` service → ports → adapters. Inference is a second hop: the daemon's `WorkerModelClient` forwards OCR/embed/generate calls to the `model-worker` over its own named pipe. Responses stream back as `ResponseEnvelope`s; the last one has `terminal = true`. Search is a stream: citation events, then answer tokens, then a `Completed` event. Screenshot bytes and model weights are **never** copied inline through IPC — assets are referenced by id/path and fetched separately (with a 16 MiB preview cap).

## Project-specific conventions & invariants

- **IPC types are generated.** `crates/ipc/build.rs` compiles `crates/ipc/proto/screensearch/v1/screensearch.proto` with a vendored `protoc` into `OUT_DIR`. To change the wire contract, edit the `.proto` and rebuild — never hand-edit generated Rust. Generated output is git-ignored and must not be committed.
- **Pipeline operations are idempotent.** Jobs are claimed with bounded leases, retries are capped (`MAX_JOB_ATTEMPTS = 5`) before dead-lettering, and capture-persist + job-enqueue happen in one transaction.
- **Model revisions are never mixed in a single search.** Every derived record stores its producing model id. A new embedding dimension gets a **new** physical vector table/index; searches pick one active revision (ADR 0002).
- **Fake providers are test-only.** `Fake*` in `model-runtime`/`windows` exist for deterministic tests; production must wire real adapters. A missing GGUF generator at `<data_dir>/models/generator/model.gguf` fails explicitly and **never** silently falls back to a fake. Evidence-only search stays fully usable with no generation model installed.
- **Automation is a manual, daemon-owned, disabled-by-default opt-in path (ADR 0004).** Model output **cannot create, approve, or execute** a plan. A plan is 1–10 exact typed actions (UIA invoke / set-Value / a bounded key chord without the Windows key / ≤512 typed Unicode scalars) against one exact HWND+PID+executable; approval is one-shot, stored as a BLAKE3 digest, and expires after 60 s; execution is single-flight with a 10 s deadline and ≥100 ms input spacing. **Before every action** the daemon re-verifies — failing closed on any unknown state — that automation is still enabled, the shell's `Ctrl+Alt+Shift+Esc` abort heartbeat is live, abort is not latched, the session is unlocked, and the foreground identity still matches the approved target. Keep all of these intact.
- **Lints are strict.** Workspace denies `unsafe_code`, warns `missing_docs`, and enables clippy `pedantic`. Public Rust APIs require doc comments. Libraries use `thiserror`; binaries use `anyhow` with context.
- **rustfmt:** edition 2024, **Windows newlines (CRLF)**, field-init shorthand. TypeScript is strict and uses functional React components.

## Documentation & data safety

- Architecture decisions: `docs/adr/`. Product/UI decisions: `docs/design/`. Performance baselines: `docs/performance/` (`P1_SCALE_BASELINE.md`, `P3_MODEL_SELECTION.md`). Visual/UI fidelity QA notes: root `design-qa.md`. The `specs/` directory is a 9-file spec pipeline (`00`–`08`) separating current truth from desired direction, plus a human-owned known-gaps register (`07_KNOWN_GAPS.md`). `AGENTS.md` at the repo root is a thin pointer back to this file, which is the single source of truth for agents and contributors.
- **Changelog updates belong in both places when relevant:** use `CHANGELOG.md` for user-facing repository changes and `specs/08_CHANGELOG_AI.md` for meaningful AI-assisted implementation notes.
- **Never commit** captures, databases, logs, secrets, model weights, generated IPC output, or any user screen data — the `.gitignore` already excludes `data/`, `models/`, `captures/`, `*.db*`, `*.log`, and `src-tauri/gen/`.
