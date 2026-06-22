# P3 Model Selection Baseline

Status: local text-only smoke measurements recorded; release model selection still open.

## Goal

Choose the default local answer-generation model for citation-constrained screen-memory answers. The model must answer only from retrieved OCR evidence, cite capture identifiers, refuse no-evidence prompts, stream acceptably on the reference Windows machine, and unload cleanly.

## Local Candidate Pool

The ignored repo-local `/models` directory currently contains development candidates including:

- `NVIDIA-Nemotron3-Nano-4B-Q4_K_M.gguf`
- `Ministral-3-3B-Reasoning-2512-Q4_K_M.gguf`
- `Ministral-3-3B-Reasoning-2512-BF16.gguf`
- `qwen3-vl-4b-instruct-q4_k_m.gguf`
- `Qwen3VL-4B-Thinking-Q4_K_M.gguf`
- `Sber_Qwen3-VL-4B-Instruct-action-Q4_K_M.gguf`

The first measured pass should prioritize text-only citation answering. Vision and `mmproj` candidates remain experimental until generation receives image asset paths by contract.

## Download Candidate Pool

Candidate HF entries to import through the app:

- `unsloth/Qwen3.5-4B-GGUF` / `Qwen3.5-4B-Q4_K_M.gguf`
- `unsloth/Ministral-3-3B-Instruct-2512-GGUF` / `Ministral-3-3B-Instruct-2512-Q4_K_M.gguf`
- `unsloth/Ministral-3-3B-Reasoning-2512-GGUF` / `Ministral-3-3B-Reasoning-2512-Q4_K_M.gguf`
- `unsloth/gemma-4-E4B-it-GGUF` / `gemma-4-E4B-it-Q4_K_M.gguf`
- `unsloth/Phi-4-mini-reasoning-GGUF` / `Phi-4-mini-reasoning-Q4_K_M.gguf`
- `unsloth/SmolLM3-3B-GGUF` / `SmolLM3-3B-Q4_K_M.gguf`

## Scoring

- 40% citation compliance and groundedness
- 15% no-evidence refusal
- 15% concise answer usefulness
- 15% first-token and full-answer latency
- 10% peak memory plus load/unload behavior
- 5% cancellation and repeated-run stability

## Required Prompt Cases

- One exact fact from one capture
- Fact spread across multiple captures
- Conflicting evidence
- No evidence
- OCR prompt-injection text
- Long evidence context near budget
- Required capture-ID citation formatting

## 2026-06-22 Local Text-Only Smoke Pass

This pass validates the default CPU llama.cpp build and supervised worker plumbing against two
ignored repo-local Q4 candidates. It does not select a release default: GAP-002/GAP-003 still own
release model approval and packaging policy, and GAP-008 still owns the memory-pressure unload
signal. Hashes below are SHA-256 because `b3sum` was not installed in the test environment.

| Model | Size | SHA-256 | Benchmark output |
|---|---:|---|---|
| `Ministral-3-3B-Reasoning-2512-Q4_K_M.gguf` | 2,147,021,472 bytes | `7e9516cc01a039bb3e2d41227cdf388849bc1c942c4624c84567b1684cd9c0fc` | `model_benchmark path=.\models\Ministral-3-3B-Reasoning-2512-Q4_K_M.gguf duration_ms=5641 token_pieces=49 status=ok` |
| `NVIDIA-Nemotron3-Nano-4B-Q4_K_M.gguf` | 2,837,072,864 bytes | `be5d9a656a51922f24f1f09a759cebb694e1f5d9728bf0ef9f8c972c5a0b5ef2` | `model_benchmark path=.\models\NVIDIA-Nemotron3-Nano-4B-Q4_K_M.gguf duration_ms=7338 token_pieces=38 status=ok` |

Worker validation passed for both candidates with:

```powershell
$env:SCREENSEARCH_RUN_WORKER_IT = "1"
$env:SCREENSEARCH_TEST_GGUF = (Resolve-Path ".\models\<candidate>.gguf").Path
cargo test -p screensearch-daemon --test worker_supervision -- --ignored --nocapture
```

Each run passed all five live cases: readiness plus lifeline exit, kill-then-restart recovery,
generation after restart, cancellation keeping health responsive, and raw idle unload releasing the
resident model.

## GPU Offload Note

GGUF generation now prefers a prebuilt llama.cpp Windows Vulkan sidecar instead of requiring a local Vulkan SDK Cargo build. On first Windows generation, the worker reuses an installed sidecar under the ScreenSearch data directory, honors `SSV2C_LLAMA_RELEASE_URL` when set to a `ggml-org/llama.cpp` release URL, or selects the newest recent release with a `*-bin-win-vulkan-x64.zip` asset. Temporarily incomplete latest releases are skipped.

```powershell
$env:SSV2C_LLAMA_RELEASE_URL = "https://github.com/ggml-org/llama.cpp/releases/tag/<tag>" # optional
cargo build -p screensearch-model-worker
```

The sidecar `llama-cli.exe` is invoked with full GPU layer offload, a 4096-token context, the current physical-core thread count, and the same generation token cap as the embedded provider. Sidecar downloads use temporary files and staging directories; zip entries are rejected if they escape the staging root. If sidecar acquisition or execution fails, diagnostics stay content-free and generation falls back to the embedded CPU provider so evidence search still works.

The table above remains a CPU baseline. Rerun the benchmark and gated worker suite on GPU-capable hardware before comparing GPU latency or selecting a release default. The `gpu-vulkan` Cargo feature remains available for advanced local-build experiments, but it is no longer the normal GPU acceleration path and still requires the Vulkan SDK on the build machine.

## Engineering Recommendation

Keep both candidates in the development pool. `Ministral-3-3B-Reasoning-2512-Q4_K_M.gguf` is the
faster smoke benchmark in this run, while `NVIDIA-Nemotron3-Nano-4B-Q4_K_M.gguf` remains a useful
hybrid-architecture comparison point. Do not choose a release default yet: the benchmark harness only
proves load/generate/unload plumbing and does not score the required groundedness, no-evidence,
conflicting-evidence, prompt-injection, or citation-format cases. GPU-capable machines may also
change relative latency once the Vulkan build is available.

## Remaining Measurements

Before item 15 can close, record for the candidate that product/legal approve:

- load time, first-token latency, full-answer latency, tokens per second, peak working set, and
  unload result from a release-like run, including GPU backend status where applicable;
- pass/fail notes for every required prompt case listed above;
- the chosen model acquisition mode and release provenance from GAP-002/GAP-003;
- the Windows memory-pressure unload policy from GAP-008.

## Development Run Note

The daemon now launches `screensearch-model-worker.exe` as a sibling executable. When running the daemon directly with Cargo, build the worker binary first because `cargo run -p screensearch-daemon` does not automatically build sibling workspace binaries:

```powershell
cargo build -p screensearch-model-worker
cargo run -p screensearch-daemon
```

Packaged builds must include the model-worker executable beside the daemon.
