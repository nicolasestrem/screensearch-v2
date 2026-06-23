# ScreenSearch V2 Known Gaps

Last updated: 2026-06-22

This file contains decisions or external inputs that cannot be safely invented by an implementation agent.

| ID | Gap | Owner | Needed by | Status |
|---|---|---|---|---|
| GAP-001 | Select one of three product visual directions for the tray/hotkey search experience. | Product owner | Before production UI implementation | Resolved: Memory Timeline |
| GAP-002 | Approve redistribution of the Apache-2.0 `Xenova/all-MiniLM-L6-v2` quantized ONNX files and approve the final selected GGUF generation model for release. Also decide how the embedding revision is pinned/verified: `fastembed` downloads the repo's `main` branch unpinned, so the persisted `revision_hash` (migration 0008) is the advertised revision, not a download-verified hash. | Product owner / legal | Before packaging model weights | Open; ignored for engineering model comparison |
| GAP-003 | Choose the release default between bundled model resources, explicit Hugging Face download, or both. | Product owner | Before release packaging | Open; engineering branch supports both acquisition paths |
| GAP-004 | Define the default retention period or disk budget shown during first-run/settings. | Product owner | Before retention defaults ship | Resolved for P1: explicit `Keep all` / `No limit` conservative default |
| GAP-005 | Provide Windows code-signing identity and release ownership. | Product owner | Before public distribution | Open |
| GAP-006 | Decide whether guarded automation belongs in the first public release or remains feature-disabled. | Product owner | Before release scope freeze | Resolved: included behind explicit default-off opt-in |
| GAP-007 | Name the reference Windows hardware profile for latency and resource budgets. | Product owner / engineering | Before performance acceptance | Resolved for P1 engineering baseline: Ryzen 7 7800X3D, 32 GB RAM, Kingston NVMe, Windows 11 Pro |
| GAP-008 | Choose the Windows memory-pressure signal that triggers an early generation-model unload (e.g. a memory-resource-notification or working-set threshold) and its policy. | Engineering | Before item 15 closes | Open; idle-timeout unload is implemented, the memory-pressure path is deferred |
| GAP-009 | Expand the deterministic source-hint vocabulary beyond Telegram, GitHub, Codex, and Amazon without turning source extraction into model inference. | Engineering | Before broad app-specific answer planning | Open; current planner intentionally covers the four acceptance prompts |
| GAP-010 | Carry the user-facing timezone/clock basis through `SearchRequest` instead of relying on the daemon's local clock. | Engineering | Before any split-user, VM, or remote-daemon deployment | Open; current supported topology is one Windows user session on one machine |
| GAP-011 | Further harden generated-answer prompts against adversarial application/window-title metadata beyond the current untrusted-evidence instruction. | Engineering / security | Before release security review | Open; OCR and metadata are treated as untrusted in the prompt, but metadata escaping/audit is still a hardening task |

## Current safe assumptions

- Development uses Windows-provided offline OCR without redistributing separate weights.
- Development uses quantized `Xenova/all-MiniLM-L6-v2` at model revision `751bff37182d3f1213fa05d7196b954e230abad9`, advertised as Apache-2.0; release redistribution still requires approval.
- The legacy GGUF adapter path `models/generator/model.gguf` remains a compatibility fallback. New engineering work uses the generation-model catalog below the app data model root, with explicit local import or Hugging Face download.
- Evidence-only search remains fully usable when generation models are absent.
- Useful local-answer planning is deterministic and uses captured screenshot/OCR evidence only: relative time phrases use the Windows local clock, source/time filters apply before ranking, and the UI shows the interpreted plan/timezone. This does not close release model approval or qualitative answer scoring.
- The model worker is daemon-supervised and live-validated with local GGUF candidates: crashes trigger sliding-window bounded restarts, a parent lifeline pipe ties the worker's lifetime to the daemon, and the resident generation model unloads after an idle timeout (a code constant) while keeping the catalog selection for lazy reload. Memory-pressure-triggered unload is deferred to GAP-008.
- GGUF generation can use opt-in Vulkan GPU offload in the model worker. The runtime asks llama.cpp whether offload is supported and otherwise uses CPU; release packaging still needs the final model/acquisition decision in GAP-002/GAP-003.
- Guarded automation is included only behind explicit default-off opt-in. It is manual, keeps persistence/audit records content-free, requires a live abort heartbeat and exact target/plan approval, and never accepts model-generated plans, mouse coordinates, clipboard actions, shell commands, elevation bypass, or Windows-key chords.
- Development backpressure uses daemon-owned high/low-water marks of 100/50 active jobs. The P1 baseline is accepted on the named GAP-007 engineering machine; release-hardware soak budgets remain a separate hardening task.
- The shell summon hotkey is a shell-local setting (default `Ctrl+Shift+Space`) stored as JSON under the Tauri app-config directory, not in the daemon archive. Window close hides to the tray; only tray Quit exits the shell. Both defaults were product-owner confirmed on 2026-06-21, and the native tray/global-hotkey runtime was user-attested as passed on 2026-06-22.
