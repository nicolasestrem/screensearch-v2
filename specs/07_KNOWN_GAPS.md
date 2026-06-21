# ScreenSearch V2 Known Gaps

Last updated: 2026-06-21

This file contains decisions or external inputs that cannot be safely invented by an implementation agent.

| ID | Gap | Owner | Needed by | Status |
|---|---|---|---|---|
| GAP-001 | Select one of three product visual directions for the tray/hotkey search experience. | Product owner | Before production UI implementation | Resolved: Memory Timeline |
| GAP-002 | Approve redistribution of the Apache-2.0 `Xenova/all-MiniLM-L6-v2` quantized ONNX files and select/approve a GGUF generation model. | Product owner / legal | Before packaging model weights | Open |
| GAP-003 | Choose whether models ship in the installer or download through an explicit first-run flow. | Product owner | Before release packaging | Open |
| GAP-004 | Define the default retention period or disk budget shown during first-run/settings. | Product owner | Before retention defaults ship | Open |
| GAP-005 | Provide Windows code-signing identity and release ownership. | Product owner | Before public distribution | Open |
| GAP-006 | Decide whether guarded automation belongs in the first public release or remains feature-disabled. | Product owner | Before release scope freeze | Open |
| GAP-007 | Name the reference Windows hardware profile for latency and resource budgets. | Product owner / engineering | Before performance acceptance | Open |

## Current safe assumptions

- Development uses Windows-provided offline OCR without redistributing separate weights.
- Development uses quantized `Xenova/all-MiniLM-L6-v2` at model revision `751bff37182d3f1213fa05d7196b954e230abad9`, advertised as Apache-2.0; release redistribution still requires approval.
- The GGUF adapter expects `models/generator/model.gguf`; a missing file fails explicitly and never falls back to a fake.
- Evidence-only search remains fully usable when generation models are absent.
- Automation remains disabled until GAP-006 is resolved and all safety tests pass.
