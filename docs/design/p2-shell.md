# ScreenSearch V2 — P2 Product Shell

Reference for the P2 "product shell" slice: the system tray, the configurable system-wide
summon hotkey, hide-to-tray window lifecycle, and complete keyboard navigation. This is the
durable description of *what was built and how to verify it*. Product/visual decisions live in
[`README.md`](README.md); ordered work lives in [`../../specs/06_PATCH_PLAN.md`](../../specs/06_PATCH_PLAN.md)
(item 14).

## Scope

Implements the master-spec product-experience requirements (`03_MASTER_PRODUCTION_SPEC.md` §12)
that the production shell is "a tray application with an at-a-glance capture state and explicit
pause/resume" and that "a configurable global hotkey opens a compact search window focused in the
query field," plus the §21 definition-of-done clause "the selected compact tray/hotkey UI is
implemented with keyboard-accessible states."

The Memory Timeline search/timeline/evidence/settings UI itself was delivered earlier (patch
items 12–13). This slice adds only the shell lifecycle and keyboard accessibility around it.

## Confirmed product decisions

| Decision | Value | Owner | Date |
|---|---|---|---|
| Default summon hotkey | `Ctrl+Shift+Space` (configurable in Settings) | Product owner | 2026-06-21 |
| Window close (X) behavior | Hide to tray; only tray **Quit** exits the shell | Product owner | 2026-06-21 |

`Ctrl+Space` alone is intentionally avoided (it collides with the Windows IME/language switch);
`Alt+Space` is avoided (it opens the window system menu).

## Architecture

The shell is the only process that knows about the tray and hotkey. The capture **daemon is a
separate process** (`apps/daemon`) reached only over the named pipe; quitting the shell never
stops capture, and the shell never opens the archive database.

### System tray (`apps/desktop/src-tauri/src/main.rs`)

- Built in the Tauri `.setup()` hook (`setup_tray`) from the bundled `icons/icon.ico` via
  `TrayIconBuilder` + `MenuBuilder`.
- Menu: **Open ScreenSearch** · *(disabled status line)* · **Pause/Resume capture** · **Quit
  ScreenSearch**.
- A `tauri::async_runtime` background task (`spawn_health_poll`) calls the same `fetch_health()`
  used by the `health` command every 3 seconds and updates:
  - the tray **tooltip** — `ScreenSearch V2 — Capturing · N queued` / `… Paused …` /
    `… Catching up …`, or `ScreenSearch V2 — Daemon offline`;
  - the disabled **status line** — `Capturing · N queued` / `Paused` / `Catching up` /
    `Daemon offline`;
  - the **Pause/Resume** menu label.
- Tray actions: **Open** and left-click → `summon_main_window` (unminimize + show + focus);
  **Pause/Resume** → `toggle_pause` reads current state via `fetch_health` and flips it with the
  existing `SetCapturePaused` IPC (same daemon policy the in-window button toggles); **Quit** →
  `app.exit(0)` (shell only).
- The poller and toggle are best-effort: if the daemon is unreachable they degrade to
  `Daemon offline` rather than panicking, and `try_state::<TrayHandles>()` avoids any
  teardown-race panic.
- Toggling capture from the tray fires a **native notification** (`tauri-plugin-notification`,
  "ScreenSearch paused/resumed") and refreshes the tray tooltip/status immediately rather than
  waiting for the next poll, so feedback is visible even when the window is hidden. Windows toast
  notifications require a registered application id, so they appear in packaged builds and may be
  suppressed under `tauri dev` — the tray tooltip/menu remain the dev-mode at-a-glance signal.

### Hide-to-tray (`main.rs`, `on_window_event`)

`WindowEvent::CloseRequested` calls `api.prevent_close()` then `window.hide()`. The window is
re-shown by the tray (**Open** / left-click) or the global hotkey. The daemon is unaffected.

### Global summon hotkey (`main.rs` + `tauri-plugin-global-shortcut`)

- The plugin is registered with a Rust-side handler; on `ShortcutState::Pressed` it calls
  `summon_main_window` and emits a `summon-search` Tauri event.
- The shortcut is registered **from Rust** (`apply_shortcut` → `app.global_shortcut().register`),
  so no global-shortcut JS capability permission is required. `apply_shortcut` unregisters any
  prior binding first, so changing the hotkey is live.
- The React UI listens for `summon-search` (guarded by `isTauri`) and focuses + selects the
  search field, so the global hotkey lands the cursor in the query box.

### Shell-local settings (`apps/desktop/src-tauri/src/shell_settings.rs`)

- `ShellSettings { hotkey: String }` is persisted as JSON at
  `<app config dir>/shell-settings.json` (Tauri `app_config_dir`, e.g.
  `%APPDATA%\com.screensearch.v2\shell-settings.json`). It is **not** in the daemon archive — the
  hotkey is a pure presentation concern, so coupling it to durable indexing state would violate
  the hexagonal layering rule.
- `load` degrades to defaults if the file is missing or corrupt (a bad file never blocks
  startup). `save` writes atomically (temp file + rename).
- Commands: `get_shell_settings` (returns the current value) and `set_shell_settings(hotkey)`
  (validates the accelerator with `Shortcut::from_str` **before** persisting, then re-registers
  the shortcut live; an invalid combination is rejected and surfaced in the Settings dialog and
  cannot brick the next launch).

## Keyboard model (`apps/desktop/src/App.tsx`, `styles.css`)

| Keys | Context | Action |
|---|---|---|
| `Enter` | search field | Run the search (the command bar also has a visible **Search** submit button) |
| `Ctrl`/`Cmd`+`K` | anywhere in the window | Focus and select the search field |
| `Arrow Up` / `Arrow Down` | evidence timeline (not while typing) | Move the selected result up/down; focus and scroll it into view; reset the detail tab to *Extracted text* |
| `Home` / `End` | evidence timeline (not while typing) | Select the first / last result |
| `Arrow Left` / `Arrow Right` / `Home` / `End` | evidence detail tabs | Move between *Extracted text* / *Metadata* / *Source* (ARIA tablist pattern) |
| `Enter` / `Space` | a focused result or tab | Activate it (native button behavior) |
| `Tab` / `Shift`+`Tab` | open dialog | Cycle focus **within** the dialog (focus trap) |
| `Escape` | open dialog | Close the dialog and restore focus to the control that opened it |
| `Escape` | search field focused, no dialog | Blur the search field so arrow keys drive the timeline |

Implementation details:

- Results use a **roving tab index** (`tabIndex = selected ? 0 : -1`) so `Tab` reaches the
  selected result and arrow keys navigate within the list. Arrow keys are ignored while an
  `INPUT`/`TEXTAREA`/`SELECT`/contentEditable element has focus (the `isTypingTarget` guard) so
  text editing and native `<select>` behavior are preserved.
- Dialogs focus their first control on open, trap `Tab` focus, and restore focus to the opener on
  close (`openModal` records `document.activeElement`; `closeModal`/Escape restores it).
- `:focus-visible` rings are added at specificity 0 (`:where(...)`) so keyboard focus is visible
  without affecting pointer interaction or overriding component styles.

## Hotkey accelerator vocabulary

One canonical string is shared by the settings file, `Shortcut::from_str`, and the UI's
`HotkeyCapture` control: `CmdOrCtrl`/`Ctrl`/`Shift`/`Alt`/`Super` joined by `+` with the main key
last (e.g. `CmdOrCtrl+Shift+Space`, `CmdOrCtrl+Shift+J`). `CmdOrCtrl` resolves to Control on
Windows. The UI displays it prettified (`Ctrl Shift Space`). An accelerator the OS rejects (busy
or invalid) surfaces an error in Settings and leaves the previous hotkey active.

## Verification

### Automated (CI-equivalent, all green)

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --workspace` | all suites passed |
| `npm run lint` (`apps/desktop`) | clean |
| `npm run build` (`apps/desktop`) | clean |

`cargo test` compiles the tray/hotkey setup but never launches the GUI, so it carries no runtime
risk and provides no runtime coverage of the tray/hotkey.

Keyboard navigation is pure frontend and was verified end-to-end with browser automation against
the dev server (`npm run dev`, preview data): arrow/Home/End selection moves the selected result
and focus and the detail pane follows; the typing guard prevents list navigation while the search
field is focused; the detail tablist responds to arrow keys; the settings dialog focuses its first
control, traps `Tab`, and restores focus on `Escape`; the hotkey-capture control records
`Ctrl+Shift+J` → `Ctrl Shift J`; and `Ctrl+K` focuses and selects the search field.

### OCR overlay alignment (review fix, 2026-06-23)

A scrupulous P2 review found that the OCR highlight overlays were positioned by **percentage of
the `.capture-image` container**, but the screenshot is rendered with `object-fit: contain` inside
fixed-aspect CSS boxes (thumbnail 16:9, large ~16:8.1 with a `max-height: 49vh` clamp). On any
capture whose aspect ratio differs from the box — ultrawide, portrait/rotated monitors, or once
the clamp engages — the image letterboxes but the overlays stayed anchored to the full container,
so the green boxes drifted off the text. This escaped the original visual QA because the dev-server
preview drew **synthetic bounds over blank space** rather than over real OCR'd text.

The overlays are now measured against the actually-rendered image rectangle (`useContainedRect`:
`useLayoutEffect` + `ResizeObserver` + `object-fit: contain` math) and positioned in pixels, in
both the timeline thumbnail and the large detail view. The preview fixture (`api.ts`) now declares
the true `qa-capture.png` dimensions (2600×1088) with bounds over real on-screen text, so browser
QA exercises alignment. Verified with Playwright against the dev server: in the large detail view
the box clamped to aspect 2.046 against the 2.39 fixture applies a 31.6 px top letterbox and the
overlays land on the text; the thumbnail applies an 8.64 px letterbox; both track across window
resizes.

### Manual Windows end-to-end runbook (required to close item 14)

The native tray, global hotkey, and hide-to-tray runtime can only be confirmed on a real Windows
desktop session. This is the `03_MASTER_PRODUCTION_SPEC.md` §18 "Verify tray pause/resume and
global hotkey" check.

Setup:

```powershell
# Terminal 1 — daemon against an isolated data dir (never touches real user data)
$env:SCREENSEARCH_DATA_DIR = "$PWD\.local-data"
cargo run -p screensearch-daemon

# Terminal 2 — the Tauri shell
cd apps/desktop
npm run tauri dev
```

Checklist (tick each; expected result in parentheses):

- [x] Tray icon appears; hovering shows a tooltip with live state and queue depth
      (`ScreenSearch V2 — Capturing · N queued`).
- [x] Click the in-window **Pause capture**; within ~3 s the tray tooltip/status shows `Paused`
      and the tray menu item reads **Resume capture**. Resume flips both back.
- [x] Tray **Pause capture** toggles the in-window capture-state pill to *Paused*; **Resume
      capture** flips it back. (Tray and window control the same daemon policy.)
- [x] Left-click the tray icon and the **Open ScreenSearch** menu item both un-hide and focus the
      window.
- [x] Tray **Quit ScreenSearch** exits the shell **only** — terminal 1 shows the daemon still
      running (capture continues).
- [x] Click the window **X**: the window hides (no exit); re-open it from the tray.
- [x] With the window hidden or behind another app, press **Ctrl+Shift+Space**: the window comes
      to the front, focused, with the search field focused and its text selected.
- [x] Settings → **Summon shortcut**: record a new combo (e.g. Ctrl+Shift+J). The old combo no
      longer summons; the new one does, immediately. Restart the shell → the new hotkey persists
      (`%APPDATA%\com.screensearch.v2\shell-settings.json`).
- [x] Record a combination the OS already owns (or an invalid one): Settings shows an error and
      the app stays stable with the previous hotkey active.
- [x] Stop the daemon (Ctrl+C in terminal 1): the tray tooltip becomes
      `ScreenSearch V2 — Daemon offline`; the shell stays alive and the window still opens.
- [ ] (Added 2026-06-23) Open a capture taken on a **non-16:9 monitor** — ideally an ultrawide
      (21:9/32:9) and a portrait/rotated display — and inspect it. The green OCR highlight boxes
      must frame the actual on-screen text with no drift, in both the timeline thumbnail and the
      large detail view, and must stay aligned as you resize the window.

The tray/hotkey/hide-to-tray items below were attested on 2026-06-22. The 2026-06-23 overlay
check is new and awaits user attestation on real non-16:9 captures (browser QA against the preview
fixture already confirmed the alignment math).

```
manual windows e2e result: passed (tray/hotkey/hide-to-tray)
tested by: user-attested   date: 2026-06-22   build: 1d88405
overlay alignment (non-16:9): pending user attestation   added: 2026-06-23
```

## Troubleshooting

- **No tray icon** — confirm `tauri = { features = ["tray-icon"] }` and that `icons/icon.ico`
  exists; the shell logs a setup error if the bundled icon is missing.
- **Hotkey does nothing** — another application or the OS may already own the combination; pick a
  different one in Settings. The shell logs (best-effort) when registration fails at startup.
- **Tray shows "Daemon offline"** — start `screensearch-daemon`; the shell intentionally keeps
  running without it (evidence search degrades, the shell does not crash).
- **Settings hotkey won't save** — the accelerator was rejected by `Shortcut::from_str`; the
  Settings dialog shows the error and keeps the previous value.
