//! Shell-local settings owned by the Tauri shell.
//!
//! These never travel to the daemon and are never stored in the archive database. The shell
//! persists them as a small JSON file under the platform application-config directory so that
//! presentation concerns (currently the global summon hotkey) stay decoupled from durable
//! indexing state, per the hexagonal layering rule that the desktop shell must not open the
//! archive database.

use std::{fs, io, path::PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// Default global summon hotkey.
///
/// `CmdOrCtrl` resolves to Control on Windows. The string is shared verbatim with
/// `tauri_plugin_global_shortcut::Shortcut` parsing and with the desktop settings UI, so all
/// three agree on one accelerator vocabulary.
pub const DEFAULT_HOTKEY: &str = "CmdOrCtrl+Shift+Space";

/// File name for the shell settings document inside the application-config directory.
const SETTINGS_FILE: &str = "shell-settings.json";

/// Settings owned entirely by the desktop shell.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellSettings {
    /// Accelerator string for the global window-summon shortcut.
    pub hotkey: String,
}

impl Default for ShellSettings {
    fn default() -> Self {
        Self {
            hotkey: DEFAULT_HOTKEY.to_owned(),
        }
    }
}

/// Resolves the shell settings path, creating the application-config directory if needed.
fn settings_path(app: &AppHandle) -> io::Result<PathBuf> {
    let directory = app
        .path()
        .app_config_dir()
        .map_err(|error| io::Error::other(error.to_string()))?;
    fs::create_dir_all(&directory)?;
    Ok(directory.join(SETTINGS_FILE))
}

/// Loads shell settings, falling back to defaults when the file is missing or unreadable.
///
/// A corrupt or partially written settings file must never prevent the shell from starting, so
/// any failure degrades to the documented defaults rather than surfacing an error.
pub fn load(app: &AppHandle) -> ShellSettings {
    let Ok(path) = settings_path(app) else {
        return ShellSettings::default();
    };
    match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => ShellSettings::default(),
    }
}

/// Persists shell settings atomically using a temporary file and rename.
pub fn save(app: &AppHandle, settings: &ShellSettings) -> io::Result<()> {
    let path = settings_path(app)?;
    let json =
        serde_json::to_vec_pretty(settings).map_err(|error| io::Error::other(error.to_string()))?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, &json)?;
    fs::rename(&temporary, &path)?;
    Ok(())
}
