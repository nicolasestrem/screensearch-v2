# ScreenSearch V2

ScreenSearch V2 is an offline-first Windows desktop application that captures screen changes, extracts text locally, indexes lexical and vector representations, and produces citation-backed answers.

This repository is a clean-room V2 implementation. It contains no legacy code, compatibility database, or migration path from ScreenSearch V1.

## Architecture

- A persistent Rust daemon owns capture, durable jobs, storage, search, and policy enforcement.
- A Tauri 2 shell hosts the React user interface and proxies typed requests to the daemon.
- A model-worker process owns OCR, embedding, and generation runtimes; the daemon supervises it — detecting crashes, restarting it within a bounded budget, and tying its lifetime to the daemon — while keeping durable archive state.
- Protobuf messages travel over local-only Windows named pipes; screen buffers and model files never cross IPC inline.
- libSQL stores relational metadata, FTS5 text, vector indexes, jobs, and transactional outbox events.
- Guarded automation is daemon-owned, disabled by default, and can emit only approved UI Automation or keyboard actions against the captured foreground target.

Architecture decisions are recorded in [`docs/adr`](docs/adr).

## Development

Prerequisites: Rust 1.88 or newer, Node.js 22 or newer, npm, and the Windows WebView2 runtime.

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cd apps/desktop
npm ci
npm run build
```

Run the bootstrap daemon with an isolated data directory:

```powershell
$env:SCREENSEARCH_DATA_DIR = "$PWD\.local-data"
cargo build -p screensearch-model-worker
cargo run -p screensearch-daemon
```

`cargo run -p screensearch-daemon` does not automatically build sibling workspace binaries. Build `screensearch-model-worker` first when running from source; packaged builds must place the worker executable beside the daemon.

GGUF generation can use GPU acceleration when the worker is built with the opt-in Vulkan backend:

```powershell
cargo build -p screensearch-model-worker --features gpu-vulkan
```

The worker checks llama.cpp GPU offload support at runtime and requests full layer offload when a compatible GPU backend is available; otherwise it falls back to CPU execution. Vulkan-enabled builds require the Vulkan SDK and runtime on the build machine.

The current vertical slice automatically captures the focused monitor, runs Windows Media OCR, produces local quantized MiniLM embeddings, and returns screenshot-backed hybrid-search evidence. Deterministic providers are test-only; optional answer generation uses a selected local GGUF model imported from disk, downloaded explicitly from Hugging Face, or discovered from packaged resources.

The desktop uses the selected **Memory Timeline** interface: search and filters lead to chronologically grouped screenshot evidence, positioned OCR highlights, provenance, and an optional cited answer. Automatic capture can be paused and resumed through a durable daemon control, with retention, storage budget, and application exclusions configured through local privacy/settings controls.

Guarded automation is a complete opt-in path but not an autonomous feature. A user must enable it after confirming the warning and live abort shortcut, capture the foreground target, review an exact typed plan, approve it, then execute the same plan before its one-shot approval expires. Supported actions are exact UIA invoke/set-value, typed key chords, and UTF-16 text input; mouse coordinates, clipboard, shell commands, Windows-key chords, and generated action plans are intentionally unsupported.

## Data safety

Never commit captures, databases, logs, API keys, model files, or other user screen data. Automation is disabled unless a structured plan is explicitly approved, the expected foreground window still matches, the interactive session is unlocked, and the emergency abort flag is clear. The automation ledger stores only content-free digests, counts, timestamps, statuses, and failure codes.
