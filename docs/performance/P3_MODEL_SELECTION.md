# P3 Model Selection Baseline

Status: implementation scaffold added; measurements pending.

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

## Pending Measurements

Record for each model:

- file name, hash, size, quantization, source
- load time
- first-token latency
- full-answer latency
- tokens per second
- peak working set
- unload result
- pass/fail for every required prompt case

## Development Run Note

The daemon now launches `screensearch-model-worker.exe` as a sibling executable. When running the daemon directly with Cargo, build the worker binary first because `cargo run -p screensearch-daemon` does not automatically build sibling workspace binaries:

```powershell
cargo build -p screensearch-model-worker
cargo run -p screensearch-daemon
```

Packaged builds must include the model-worker executable beside the daemon.
