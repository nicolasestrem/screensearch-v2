# ScreenSearch V2 Project Intake

Last updated: 2026-06-21

## Request

Build a clean-room, offline-first Windows desktop application that continuously captures eligible screen changes, extracts text locally, indexes lexical and semantic representations, and lets one user find and question their screen history with visible citations.

## Primary user

A single Windows user searching their own local screen history. There is no account, cloud tenant, team workspace, or remote administration surface in V2.

## User outcome

The user can remember something imperfectly, open ScreenSearch with a global hotkey, enter a phrase or natural-language query, and receive the matching screen moments with enough visual evidence to verify every result.

## Product principles

1. Evidence before synthesis: screenshots, time, application, window title, OCR text, and matching regions are primary; generated prose is secondary.
2. Local by default: captures, extracted text, embeddings, models, and generated answers stay on the device.
3. Quiet operation: capture and indexing are automatic and observable without becoming a dashboard the user must operate.
4. Explicit privacy: pause, exclusions, retention, deletion, and storage location are understandable and controllable.
5. Bounded automation: no input is emitted without an approved plan, foreground validation, and an emergency abort path.

## Requested delivery sequence

1. Real Windows capture with automatic durable job processing.
2. Real local OCR with evidence-rich visual search results.
3. Real local embeddings with hybrid retrieval.
4. A tray and global-hotkey search experience based on real data.
5. Local citation-constrained generation and guarded automation.

## Constraints

- Repository: `C:\Users\nicol\Documents\GitHub\screensearch-v2`.
- Clean-room V2; no V1 source, history, schema, compatibility code, or migration.
- Rust 2024 workspace, Tauri 2, React, TypeScript, and Vite.
- Windows-only initial release; other operating systems are future work.
- Offline, single-user operation; no API keys are required for core behavior.
- Screen data, databases, logs, downloaded models, and generated IPC files must never be committed.
- The current interface is a diagnostics harness, not an approved visual target.

## Success test for the first truthful slice

Display a distinctive phrase in an ordinary Windows application, allow ScreenSearch to capture and index it without manual job controls, search for the phrase, and receive the correct screenshot with application, timestamp, excerpt, and matching region in under one second on the reference development machine.

## Non-goals for the first release

- Cloud synchronization, collaboration, accounts, or telemetry collection.
- macOS or Linux capture and automation.
- V1 data import.
- Autonomous OS actions.
- Training or fine-tuning models.
- A conversational assistant that can answer without retrieved evidence.
- Pixel-perfect visual design before real capture and retrieval data exist.

## Open product decision

The final visual direction is intentionally undecided. Before production UI implementation, generate three grounded directions for the confirmed compact tray/hotkey workflow and record the selected direction in `UI_REFERENCE.md` or this file.
