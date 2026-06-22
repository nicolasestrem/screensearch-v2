# ScreenSearch V2 Master Production Specification

Version: 0.2-draft  
Last updated: 2026-06-21  
Status: implementation contract

This document answers one question: **exactly how should ScreenSearch V2 be built?** If this specification is silent or contradictory on a production-significant decision, stop and record the ambiguity rather than inventing a default.

## 1. Scope and invariants

ScreenSearch V2 is an offline, single-user Windows desktop application. Its core path is:

```text
eligible Windows frame
  -> privacy policy
  -> change/dedup policy
  -> immutable image asset + capture row + durable job
  -> local OCR + chunking + local embedding
  -> FTS5 + vector indexes
  -> hybrid evidence results
  -> optional local cited generation
```

Mandatory invariants:

- The daemon is the only writer of archive state.
- Capture persistence and job creation are atomic.
- Job completion and derived-data writes are atomic and idempotent.
- One query never mixes embedding model revisions.
- Captured pixels, OCR text, prompts, and model output are never sent over the network.
- Search evidence is emitted before generated answer tokens.
- No generated answer is shown as grounded when retrieval returned no evidence.
- Automation is disabled by default and cannot bypass approval, foreground, timeout, rate, or abort gates.

## 2. Component architecture

### 2.1 Desktop shell

Tauri 2 owns window creation, tray behavior, a configurable global hotkey, app lifecycle, native notifications, and typed proxy commands. React owns presentation and ephemeral interaction state. The desktop shell must not open the archive database or model files.

### 2.2 Daemon

The Rust daemon owns capture scheduling, policy, assets, libSQL, job scheduling, search orchestration, worker supervision, retention, deletion, and audit events. It starts independently of the search window and continues while the tray process is active.

### 2.3 Model worker

OCR, embeddings, vision, and generation execute outside the UI process. A worker crash must fail or release the leased job without corrupting committed state. Heavy models load lazily. Only model revision, asset path, bounded text, vectors, and streamed tokens cross the worker boundary; image bytes do not travel inline.

### 2.4 Persistence

libSQL stores relational metadata, FTS5, vector indexes, jobs, policies, and outbox events. Immutable encoded images live below the platform data directory in content-addressed paths. Model weights live in a separate models directory and are not archive assets.

## 3. Platform directories

Defaults below `%LOCALAPPDATA%\ScreenSearchV2`:

```text
screensearch.db
assets/<hash-prefix>/<blake3>.png
models/<provider>/<revision>/...
logs/                      # metadata only; no OCR text, prompts, or pixels
```

`SCREENSEARCH_DATA_DIR` may override the root for development and tests. Asset paths persisted in the database are relative to the asset root. Files must be created with current-user access only where Windows APIs permit it.

## 4. Capture

### 4.1 Initial adapter

Implement a Windows desktop capture adapter behind `CaptureSource`. The first production-capable version may use a Win32 desktop bitmap capture for broad compatibility; Windows Graphics Capture is the preferred follow-up where it improves multi-monitor, protected-content, or performance behavior. The adapter returns a PNG or another explicitly versioned encoded image format, never an undocumented raw byte buffer.

Each frame includes UTC capture time, stable monitor identifier, foreground process name, privacy-filtered foreground title, physical dimensions, media type, and bytes.

### 4.2 Cadence and change policy

- Default cadence: one eligibility check every 2 seconds.
- Only one capture operation may run at a time.
- Exact BLAKE3 duplicates are discarded by the existing unique fingerprint.
- A perceptual-change policy is required before increasing the default cadence.
- When the durable queue reaches its configured high-water mark, capture pauses and reports `backpressured` rather than allowing unbounded growth.
- Capture resumes automatically below the low-water mark.
- The initial queue thresholds are 100 active jobs for high-water and 50 active jobs for low-water. They remain daemon-owned constants until versioned user settings land.
- The first perceptual policy compares a 32 by 18 grayscale Triangle-filtered signature with the last accepted frame on the same monitor. A frame is skipped only when dimensions, foreground application, and title are unchanged, mean sample delta is at most 2/255, and at most 1 percent of samples differ by more than 12/255. Skipped frames do not move the baseline, so cumulative changes eventually persist; the first frame after restart is accepted.

### 4.3 Privacy policy

Before asset persistence, reject frames when capture is paused, the session is locked, the active application or title matches an exclusion, or ScreenSearch itself is the foreground window unless diagnostics explicitly opt in. ScreenSearch's own application identifier is excluded in the production composition. User-configurable exclusions are stored locally as case-insensitive process and title patterns and update the live daemon policy without a restart. Password-field redaction is future work and must not be claimed before implemented.

## 5. Assets and deduplication

- Encode first-release captures as PNG with media type `image/png`.
- Hash the exact encoded bytes with BLAKE3.
- Store with create-if-absent semantics.
- If database insertion fails after a new file write, orphan cleanup may remove unreferenced files after a grace period; it must never delete referenced assets.
- Deleting the final capture reference makes an asset eligible for cleanup.
- Near-duplicate behavior, when added, records why a frame was skipped and never aliases distinct visible evidence without a deterministic threshold.

## 6. Durable jobs

- The daemon runs a background analysis loop; no user-facing `Process jobs` command is required.
- A successful claim sets `status=running`, increments `attempt`, and assigns a lease owner and expiry.
- Lease duration must exceed the measured p99 OCR+embedding duration and be renewable for long operations.
- Expired running jobs are reclaimable.
- Transient failures use bounded exponential backoff with jitter.
- Maximum attempts default to 5, after which the job becomes `dead` and a `dead_letter` row is written.
- Queue depth, oldest pending age, job latency, retry count, and dead-letter count are observable without logging screen content.

## 7. OCR

### 7.1 First real provider

Use an offline Windows OCR provider or an ONNX Runtime-backed provider behind `OcrEngine`. The chosen provider must read the immutable image asset, return text in reading order, normalized bounds, language where available, confidence where available, and a stable provider/model revision. If the platform API does not expose confidence, persist a documented sentinel or nullable value through a migration; do not fabricate confidence.

### 7.2 Blocks and chunks

- Preserve word/line geometry sufficient to highlight a result.
- Normalize line endings and Unicode but preserve human-readable text.
- Chunk by coherent adjacent blocks with a bounded character/token size and enough overlap for semantic continuity.
- Empty OCR produces a successful analysis with no search chunks, not an infinite retry.
- Model upgrades create new derived data under a new revision and switch atomically after re-indexing.

## 8. Embeddings and vector search

### 8.1 Provider

Use a compact local sentence embedding model with an explicit manifest containing provider, model name, revision/hash, dimensions, tokenizer revision, pooling method, normalization, license, and source URL. The initial model should prioritize low memory and CPU latency; ONNX Runtime is the intended native runtime.

### 8.2 Storage

Vector tables are dimension-specific. A migration creates a table and index for each supported dimension, and `embedding_model` identifies the active revision. Never coerce or truncate vectors to an existing table.

### 8.3 Retrieval

- Lexical retrieval uses parameterized FTS5 queries and handles punctuation-only or invalid MATCH syntax safely.
- Semantic retrieval filters by model revision.
- Fuse ranked lexical and semantic lists with reciprocal-rank fusion.
- Deduplicate multiple chunks from the same capture for the first result view while preserving best-match geometry.
- Default result limit is 20 evidence cards; generation context uses a smaller configurable top-k.
- Exact phrase matches receive a deterministic ranking boost.
- For the active model revision, archives with at most 50,000 embeddings use deterministic exact cosine ordering; larger archives use the libSQL vector index. Both paths preserve the same evidence contract and model-revision filter.

## 9. Evidence result contract

Every citation/result returned to the UI must contain:

- capture and chunk identifiers;
- capture timestamp;
- application and privacy-filtered window title;
- screenshot asset locator exposed through a narrow Tauri command, not an arbitrary filesystem path;
- width and height;
- excerpt and score;
- one or more matching normalized bounding boxes when available;
- lexical, semantic, or fused match provenance;
- embedding and OCR model revisions for diagnostics.

The UI must be able to render results before any generator is available.

## 10. IPC

Keep protobuf over the local Windows named pipe. All envelopes carry a request identifier. Streaming responses terminate explicitly. Add versioned messages rather than changing field meaning.

Required V2 operations:

- health/status, including capture state and queue depth;
- pause/resume capture;
- search evidence with optional answer generation;
- retrieve an authorized asset by capture identifier;
- update exclusions and retention settings;
- delete captures by identifier or time range;
- subscribe to bounded status events;
- approve, execute, and abort typed automation plans when that feature is enabled.

Reject oversized frames, queries, and event streams with structured errors. Disconnect must cancel downstream generation where safe.

Health responses expose `capturing`, `paused`, or `backpressured` capture state plus queue depth, high-water mark, oldest pending age, retry count, and dead-letter count. Capture attempts that policy rejects return a content-free skip reason without a capture identifier.

## 11. Search and generation

Search has two explicit modes:

1. `evidence_only` returns ranked evidence and completes without a generator.
2. `answer` emits the same evidence, then optional answer tokens, then completion.

Generation uses a local quantized llama.cpp-compatible model behind `TextGenerator`. The prompt includes only bounded retrieved evidence and stable citation identifiers. If no hits pass the configured threshold, answer mode returns a no-evidence completion and does not call the generator. Model loading is lazy, only one generation runs by default, cancellation is propagated, and the model unloads after an idle timeout or memory-pressure signal.

## 12. Product experience

The production shell is a tray application with an at-a-glance capture state and explicit pause/resume. A configurable global hotkey opens a compact search window focused in the query field.

Primary flow:

1. Open search.
2. Enter a phrase or natural-language memory.
3. See screenshot-first evidence immediately.
4. Navigate results with keyboard or pointer.
5. Inspect a result with highlighted matching text and surrounding timeline.
6. Optionally request a cited answer.

Manual capture and queue buttons are diagnostics-only and excluded from the production surface. The production visual target must be selected from three grounded directions before UI implementation and recorded in a durable reference file.

## 13. Retention and deletion

- The conservative default is `Keep all` with no captured-asset disk limit. Settings must display that state explicitly until the user chooses an age or budget.
- Support a disk budget and/or age-based retention without deleting captures needed by an active operation.
- The disk budget counts immutable captured image assets referenced by captures; it does not claim to cap database or model-file size.
- Deletion removes derived rows transactionally and schedules unreferenced asset cleanup.
- `Delete all data` requires explicit confirmation, stops capture, closes active readers, removes database/assets/models according to the selected scope, and reports partial failure.
- The currently implemented captured-history scope requires explicit confirmation, pauses capture, removes captures and derived evidence transactionally, and performs durable idempotent asset cleanup. Database/model factory reset remains a separate release-hardening scope.
- There is no cloud backup assumption.

## 14. Automation safety

Automation remains feature-gated and off by default. Plans contain only typed, deterministic actions. Free-form model text is never passed directly to Windows input APIs.

Execution requires:

1. a persisted approval for the exact normalized plan;
2. matching foreground process and window identity immediately before each action;
3. a short execution timeout and bounded action count;
4. rate limiting;
5. an emergency abort hotkey registered outside the target application;
6. abort checks between actions;
7. a final audit record without sensitive content.

Prefer Windows UI Automation for semantic controls. `SendInput` is a fallback. Any focus change, abort, timeout, lock, or plan mismatch stops execution safely.

### 14.1 V1 action and target contract

The user manually creates an `AutomationPlanV1` for one target containing the captured HWND,
process identifier, executable file name, and display title. A plan contains one to ten actions:

- `UiaInvoke` with one exact Automation ID;
- `UiaSetValue` with one exact Automation ID and value;
- `KeyChord` with typed modifier and key enums; or
- `TypeText` with Unicode text.

Selectors and text values contain 1–512 Unicode scalar values. Mouse coordinates, clipboard,
shell execution, arbitrary key codes, Windows-key chords, and model-generated plans are rejected.
UI Automation actions do not fall back implicitly; keyboard/text behavior must be an explicit
action in the reviewed plan.

### 14.2 Approval and execution contract

The daemon computes the canonical BLAKE3 digest over the versioned normalized plan and persists
only the digest and content-free metadata. Approval is one-shot and expires after 60 seconds.
Execution is single-flight, limited to ten seconds, and spaces emitted input by at least 100
milliseconds.

The desktop shell owns the fixed `Ctrl+Alt+Shift+Esc` global abort registration and sends a
content-free heartbeat every three seconds. A heartbeat older than ten seconds blocks approval and
execution. Abort is latched until the user explicitly resets it.

Before every action the daemon verifies enablement, heartbeat freshness, abort state, known
unlocked session state, exact foreground HWND/PID/executable identity, deadline, and pacing.
Unknown lock or target state fails closed.

### 14.3 Automation storage and failures

Automation is disabled in durable settings by default. The V1 ledger stores only approval/run ID,
plan digest, action count, status, timestamps, expiry, and one stable failure code. It never stores
titles, executable paths, target handles, UI Automation selectors, set values, typed text, key
sequences, or plan JSON. Startup converts orphaned running rows to `aborted`.

Stable failures are `disabled`, `abort_unavailable`, `abort_active`, `approval_missing`,
`approval_expired`, `plan_mismatch`, `target_changed`, `session_locked`, `rate_limited`, `timeout`,
`input_blocked`, `control_missing`, `control_ambiguous`, and `control_unsupported`.

The complete P4 contract is recorded in `docs/design/p4-guarded-automation.md` and ADR 0004.

## 15. Configuration

Production-significant settings must have one documented owner and default. Do not invent environment variables ad hoc.

Approved development variables:

- `SCREENSEARCH_DATA_DIR`: archive root override.
- `RUST_LOG`: standard Rust tracing filter; logs remain content-free.

User settings belong in a versioned local settings record or file and include capture enabled state, cadence, exclusions, retention, disk budget, hotkey, language preferences, active model revisions, and generation enabled state.

## 16. Logging and diagnostics

Structured logs may include operation identifiers, durations, dimensions, byte counts, model revisions, queue metrics, and error categories. They must not include pixels, OCR text, search queries, prompts, generated answers, window titles, or full filesystem paths by default. A user-enabled diagnostic mode may expand metadata only after an explicit warning; it still excludes screen content.

## 17. Security

- Named-pipe access is restricted to the current interactive user.
- No listener binds to TCP in the default build.
- No runtime network access is required after optional, explicit model acquisition.
- Model manifests include cryptographic hashes and licenses.
- Asset retrieval validates capture identifiers and never accepts arbitrary paths.
- UI content is rendered as text; OCR and generated content are never injected as HTML.
- Dependencies and packaged binaries are scanned in CI.

## 18. Testing

### Unit and contract

- Every port has deterministic contract tests.
- Every protobuf request, response, streaming terminal state, and structured error has a round-trip test.
- Geometry, chunking, model dimension, and privacy rules have boundary tests.

### Integration

- Capture commit failure and orphan handling.
- Duplicate capture behavior.
- Lease expiry and daemon restart recovery.
- Worker crash, retry, dead-letter, and cancellation.
- Queue saturation and capture backpressure.
- UI/daemon independent restart.
- Model revision switch without mixed queries.
- Empty OCR and no-evidence search.
- Retention and deletion recovery.

### Windows end-to-end

- Capture a synthetic test window containing known text.
- OCR and find that text.
- Render the correct screenshot and matching region.
- Verify tray pause/resume and global hotkey.
- Verify automation approval, focus change rejection, timeout, and emergency abort without operating unrelated applications.

### Performance

- Synthetic metadata/index benchmark targets 10 million captures.
- Record indexing throughput, query p50/p95/p99, database size, and peak memory.
- Record idle and active capture CPU, queue latency, OCR latency, embedding latency, and model load/unload memory.
- Performance claims require a named machine profile and reproducible command.
- The accepted P1 engineering baseline is recorded in `docs/performance/P1_SCALE_BASELINE.md`; it does not replace later release-hardware soak measurements.

## 19. CI/CD and packaging

Windows CI must run Rust formatting, Clippy with warnings denied, workspace tests, frontend install/lint/build, security audit, and Tauri packaging. Generated protobuf output, runtime data, captures, and model weights are not committed. Release artifacts are versioned and checksummed. Signing is required before public distribution but is a known gap until credentials and release ownership are supplied.

## 20. Failure and rollback behavior

- Schema changes are forward-only. A failed migration prevents daemon startup and preserves the prior database for inspection.
- Derived OCR/vector data can be rebuilt from immutable assets.
- A failed model activation leaves the prior active revision usable.
- Worker failure cannot terminate the UI or corrupt a committed capture.
- UI disconnect does not cancel durable indexing.
- Generator cancellation does not remove evidence results.
- Capture failure surfaces degraded status and retries with bounds; it does not busy-loop.

## 21. Definition of done

The V2 slice is done when:

- real Windows captures are produced automatically and stored as valid images;
- real local OCR produces positioned text;
- durable analysis runs without manual pumping and recovers after restart;
- exact and semantic queries return real screenshot evidence with metadata and highlights;
- the selected compact tray/hotkey UI is implemented with keyboard-accessible states;
- optional local generation produces only citation-backed answers and has a no-evidence state;
- automation remains disabled or passes all specified approval and abort tests;
- privacy, retention, deletion, and content-free diagnostics are implemented;
- required tests and Windows packaging pass;
- `05_BUILD_REVIEW.md`, `06_PATCH_PLAN.md`, `07_KNOWN_GAPS.md`, and `08_CHANGELOG_AI.md` accurately describe the delivered build.
