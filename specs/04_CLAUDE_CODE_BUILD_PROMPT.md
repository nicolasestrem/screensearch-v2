# ScreenSearch V2 Agent Build Prompt

This file orchestrates any coding agent. It does not replace or repeat the production specification.

## Mandatory reading order

Before changing code, read:

1. `AGENTS.md`
2. `specs/00_PROJECT_INTAKE.md`
3. `specs/01_PROJECT_CONTEXT.md`
4. `specs/02_STRATEGIC_PLAN.md`
5. `specs/03_MASTER_PRODUCTION_SPEC.md`
6. `specs/05_BUILD_REVIEW.md`
7. `specs/06_PATCH_PLAN.md`
8. `specs/07_KNOWN_GAPS.md`
9. the ADRs and code relevant to the selected phase

## Source-of-truth map

- Current facts: `01_PROJECT_CONTEXT.md` and the repository at the current commit.
- Product direction and priority: `02_STRATEGIC_PLAN.md`.
- Production behavior and contracts: `03_MASTER_PRODUCTION_SPEC.md`.
- Current implementation delta: `05_BUILD_REVIEW.md`.
- Ordered remaining work: `06_PATCH_PLAN.md`.
- Decisions requiring a human or external input: `07_KNOWN_GAPS.md`.
- Why an AI-assisted change was made: `08_CHANGELOG_AI.md`.

When documents disagree with code, do not silently choose. Verify whether the document describes current state or desired state, then update the appropriate review/gap document.

## Implementation order

Follow the active ordered items in `06_PATCH_PLAN.md`. Do not begin optional generation, advanced automation, or multi-OS work before the evidence loop and its tests pass.

## Operating rules

1. Preserve dependency direction: domain and ports never import adapters, Tauri, IPC, persistence, models, or Windows APIs.
2. Keep all pipeline operations idempotent.
3. Add a migration for every schema change; never edit an already released migration to disguise drift.
4. Add or update protobuf fields/messages without changing the meaning of existing field numbers.
5. Do not add environment variables unless `03_MASTER_PRODUCTION_SPEC.md` names them or the spec is updated first.
6. Do not log pixels, OCR text, queries, prompts, answers, window titles, or sensitive paths.
7. Do not add cloud calls, telemetry, accounts, V1 compatibility, or autonomous actions.
8. Do not use destructive Git commands or overwrite unrelated user changes.
9. Keep native and unsafe code narrowly contained, documented, and reviewed. If the workspace lint forbids required unsafe Win32 calls, create the smallest adapter-scoped exception with safety comments and tests rather than weakening the whole workspace.
10. Use fake providers only in deterministic tests. Production composition must never silently fall back to a fake provider.
11. Run focused tests during development and the full required verification before claiming completion.
12. Update `05_BUILD_REVIEW.md`, `06_PATCH_PLAN.md`, `07_KNOWN_GAPS.md`, and `08_CHANGELOG_AI.md` after every meaningful build pass.

## Ambiguity stop rule

If the production spec is silent on a decision that affects persisted data, public IPC, privacy, security, deletion, automation, model licensing, packaging, or user-visible product direction:

1. stop that affected work;
2. add a concise item to `06_PATCH_PLAN.md` when the specification needs elaboration;
3. add an item to `07_KNOWN_GAPS.md` when a human decision or external dependency is required;
4. ask the user rather than guessing;
5. continue only independent, non-conflicting work.

## Completion report

Report the user-visible outcome first, then changed contracts, verification commands/results, remaining gaps, and the next highest-priority action. Never describe a placeholder as production functionality.
