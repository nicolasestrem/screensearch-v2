# ScreenSearch V2 Product UI Reference

The product owner selected **Memory Timeline** (direction 2) on 2026-06-21.

Visual source: [`memory-timeline-reference.png`](memory-timeline-reference.png)

## Binding product decisions

- ScreenSearch is a compact Windows utility, not a dashboard or marketing surface.
- Search spans the top of the window and evidence is grouped chronologically in the left pane.
- The selected screenshot is the dominant artifact; OCR text, metadata, provenance, and any generated answer remain subordinate.
- OCR bounds are drawn on the original capture rather than reconstructed content.
- The palette is near-black with neutral separators, amber selection, and green capture/OCR states.
- Controls must reflect implemented behavior. Invented frame totals, retention status, exclusions, and model availability are prohibited.

## Truthful implementation deviations

- Live ultra-wide captures use `object-fit: contain` so evidence is never cropped; this can letterbox the inspector.
- Result totals are the current query's real match count, not a fabricated archive-wide total.
- When no GGUF model is installed, the answer panel offers generation and states the requirement instead of fabricating an answer.
- Retention and application exclusions appear only in the privacy dialog with explicit not-configured language.

The blocking visual review is recorded in [`design-qa.md`](../../design-qa.md).

## Shell lifecycle (P2)

The full P2 shell reference and manual Windows verification runbook is in
[`p2-shell.md`](p2-shell.md).

- ScreenSearch runs as a tray application. The tray shows at-a-glance capture state in its
  tooltip and an informational status line, and offers **Open ScreenSearch**, **Pause/Resume
  capture**, and **Quit**. The tray status is refreshed by a background daemon health poll and
  reads `Daemon offline` when the daemon is not running.
- Closing the window **hides it to the tray**; the shell keeps running and stays summonable. Only
  the tray **Quit** exits the shell. The capture daemon is a separate process and is unaffected by
  shell quit.
- A configurable system-wide hotkey (default `Ctrl+Shift+Space`) brings the window to the front
  and focuses the search field. The hotkey is a shell-local setting persisted by the Tauri shell
  (not in the daemon archive); it is editable in Settings.

## Keyboard model (P2)

- `Ctrl/Cmd+K` focuses and selects the search field from anywhere in the window.
- In the evidence timeline, `Arrow Up/Down` move the selected result, `Home`/`End` jump to the
  first/last; selection moves focus and scrolls the item into view. Arrow keys are ignored while a
  text field, list box, or modal has focus. Results use a roving tab index so `Tab` reaches the
  selected result and arrows navigate within.
- The evidence detail tabs follow the ARIA tablist pattern (`Arrow Left/Right`, `Home`/`End`).
- Dialogs trap `Tab`/`Shift+Tab` focus, focus their first control on open, close on `Escape`, and
  restore focus to the control that opened them.
