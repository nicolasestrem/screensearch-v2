# Useful Local Answers

Status: implemented for deterministic planning, filtered retrieval, plan IPC, prompt metadata, and guided settings. Release model selection and release hardening remain tracked separately.

## Contract

ScreenSearch answers only from captured screenshots, OCR text, and archive metadata. It must not use Telegram, GitHub, Amazon, web search, account APIs, or general knowledge to answer factual questions. When evidence is missing or incomplete, the generated answer must say there is not enough local evidence.

Search stores capture timestamps in UTC. Query planning interprets relative time phrases using the host Windows local clock and converts filters back to UTC before persistence queries. UI events include the interpreted plan and timezone label so the shell can show what was understood.

## Deterministic Planner

`screensearch_application::plan_search` extracts:

- `today`: local 00:00 to next-day 00:00
- `around noon`: local 11:00 to 13:00
- `early afternoon`: local 12:00 to 15:00
- `this afternoon`: local 12:00 to 18:00
- source hints from visible app/title intent: Telegram, GitHub, Codex, Amazon

The planner leaves only useful evidence terms in `retrieval_query`. Examples:

- Telegram around noon: metadata-only retrieval, source `telegram`, 11:00-13:00 local
- largest GitHub PR today: retrieval `largest pr`, source `github`, today
- Codex settings early afternoon: retrieval `settings`, source `codex`, 12:00-15:00 local
- Amazon book this afternoon: retrieval `book`, source `amazon`, 12:00-18:00 local

The source vocabulary is deliberately small and deterministic for this pass. GAP-009 tracks expanding it beyond the four acceptance sources.

## Retrieval

`ArchiveRepository::hybrid_search` accepts `SearchFilters` and applies time/source constraints inside lexical and semantic candidate SQL before rank fusion. Filtered semantic search uses the exact vector-distance path instead of global `vector_top_k`, because global top-k would rank before source/time constraints.

FTS now uses a safe phrase clause plus OR fallback over normalized useful terms. A noisy extra term should not suppress a partial lexical match. Exact full-text matches still receive an additive boost, and matching useful terms receive a smaller additive boost. Capture-level dedupe and embedding model-revision isolation remain unchanged.

Metadata-only searches still run semantic ranking over the filtered candidate set, using source hints or the original query only to build the embedding vector.

Source hints are extracted as known whole words, then matched as escaped substrings against application names, window titles, and OCR chunk text so variants like `telegram-desktop.exe` still match and browser pages remain eligible when only the captured page text contains the site name.

## Answer Prompt

Answer prompts include, for each bounded evidence item:

- capture id
- local timestamp and timezone label
- application
- window title
- OCR excerpt

The prompt states that OCR text and capture metadata are untrusted evidence, forbids external lookup, requires capture-id citations, and requires uncertainty when local captures do not show the requested fact. Every returned citation is included in the answer context, while OCR excerpts are capped per hit so one large chunk cannot consume the answer model context. For largest-PR questions, changed-file/addition/deletion evidence must be visible; otherwise the answer must say it cannot determine the largest PR from local captures.

The desktop strips `<think>...</think>` spans before rendering generated text.

## IPC And UI

Search streams now begin with an additive `SearchPlan` event before citations. The desktop displays the interpreted source/time/retrieval plan in the filter bar and shows readiness in Settings:

- captures
- OCR chunks
- embedding chunks
- active answer model
- queue/dead-letter state
- timezone basis

Settings no longer prefill speculative model defaults. The Answer Model panel starts with local GGUF import, keeps explicit Hugging Face download under an advanced section, and exposes the installed/active model catalog. Storage settings show the active retention and asset-budget policy, with a conservative reset to `Keep all` and `No limit`.

## Content-Free Smoke

The opt-in local archive smoke does not print answer text or OCR excerpts:

```powershell
$env:SCREENSEARCH_DATA_DIR = "C:\path\to\ScreenSearchData"
cargo test -p screensearch-persistence local_archive_answer_smoke_is_content_free --test live_windows_archive -- --ignored --nocapture
```

Each line includes prompt label, interpreted filters, citation count, evidence-only/no-evidence status, latency, and whether required citations are present.
