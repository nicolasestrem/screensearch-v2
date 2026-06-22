# ADR 0004: Automation safety

## Status

Accepted

## Decision

Automation V1 is a manual, daemon-owned workflow and is disabled by default. Model output
cannot create, approve, or execute a plan.

Plans target one exact HWND, process identifier, and executable name and contain between one and
ten typed actions:

- invoke the unique descendant with an exact UI Automation ID;
- set the writable Value pattern of that unique descendant;
- emit a bounded key chord without the Windows key; or
- type at most 512 Unicode scalar values.

The daemon persists a canonical BLAKE3 digest rather than the plan contents. Approval is one-shot
and expires after 60 seconds. Execution is single-flight, has a ten-second deadline, and spaces
emitted input by at least 100 milliseconds.

Immediately before every action the daemon verifies that automation remains enabled, the desktop
shell's fixed `Ctrl+Alt+Shift+Esc` abort registration has a live heartbeat, abort is not latched,
the interactive session is known to be unlocked, and the foreground HWND/PID/executable identity
still matches the approved target. Any unknown state fails closed.

Windows UI Automation is used only for explicit UIA actions. It never silently falls back to
keyboard input. `SendInput` is used only for explicit keyboard and text actions, detects partial
injection, releases modifiers, and does not attempt to bypass UIPI or elevation boundaries.

Audit rows contain only identifiers, the canonical digest, action count, timestamps, status, and a
stable content-free failure code. They never contain titles, executable paths, control IDs, text
values, or typed text.

## Consequences

The desktop shell must keep the abort shortcut registered and heartbeat the daemon while
automation is available. Losing the shell, locking the session, changing focus, or restarting
during a run stops or recovers the operation as aborted. Mouse-coordinate, clipboard, shell,
model-generated, and multi-window actions are outside V1.
