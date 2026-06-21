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
