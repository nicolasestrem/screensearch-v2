//! Tauri shell and typed proxy to the persistent ScreenSearch daemon.

mod shell_settings;

use std::time::Duration;

use screensearch_ipc::{
    IpcError,
    transport::{DEFAULT_PIPE_NAME, IpcClient},
    v1::{
        CaptureRequest, DeleteCapturesRequest, GetArchiveSettingsRequest, GetCaptureAssetRequest,
        HealthRequest, ProcessJobsRequest, RequestEnvelope, SearchRequest, SetCapturePausedRequest,
        UpdateArchiveSettingsRequest, request_envelope, response_envelope, search_event,
    },
};
use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, WindowEvent, Wry,
    ipc::Channel,
    menu::{MenuBuilder, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthStatus {
    version: String,
    status: String,
    capture_paused: bool,
    capture_state: String,
    queue_depth: u64,
    oldest_pending_age_seconds: u64,
    retry_count: u64,
    dead_letter_count: u64,
    queue_high_water: u64,
    capture_count: u64,
    asset_bytes: u64,
    ocr_block_count: u64,
    search_chunk_count: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ArchiveSettings {
    retention_days: Option<u32>,
    disk_budget_bytes: Option<u64>,
    excluded_applications: Vec<String>,
    excluded_titles: Vec<String>,
    capture_count: u64,
    asset_bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsUpdateResult {
    settings: ArchiveSettings,
    captures_deleted: u64,
    assets_scheduled: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeleteResult {
    captures_deleted: u64,
    assets_scheduled: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptureResult {
    capture_id: String,
    duplicate: bool,
    skipped_reason: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NormalizedRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptureAsset {
    media_type: String,
    content: Vec<u8>,
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[allow(clippy::large_enum_variant)]
enum SearchUiEvent {
    Citation {
        capture_id: String,
        chunk_id: String,
        excerpt: String,
        score: f64,
        captured_at: String,
        application: String,
        window_title: String,
        width: u32,
        height: u32,
        bounds: Vec<NormalizedRect>,
        match_kind: String,
        ocr_model_id: String,
        embedding_model_id: String,
    },
    Token {
        text: String,
    },
    Completed {
        citation_count: u32,
    },
}

#[tauri::command]
async fn health() -> Result<HealthStatus, String> {
    fetch_health().await
}

/// Queries the daemon health endpoint once; reused by the `health` command and the tray poller.
async fn fetch_health() -> Result<HealthStatus, String> {
    let responses = request(request_envelope::Body::Health(HealthRequest {})).await?;
    for response in responses {
        match response.body {
            Some(response_envelope::Body::Health(status)) => {
                return Ok(HealthStatus {
                    version: status.version,
                    status: status.status,
                    capture_paused: status.capture_paused,
                    capture_state: status.capture_state,
                    queue_depth: status.queue_depth,
                    oldest_pending_age_seconds: status.oldest_pending_age_seconds,
                    retry_count: status.retry_count,
                    dead_letter_count: status.dead_letter_count,
                    queue_high_water: status.queue_high_water,
                    capture_count: status.capture_count,
                    asset_bytes: status.asset_bytes,
                    ocr_block_count: status.ocr_block_count,
                    search_chunk_count: status.search_chunk_count,
                });
            }
            Some(response_envelope::Body::Error(error)) => return Err(error.message),
            _ => {}
        }
    }
    Err("daemon returned no health response".to_owned())
}

/// Returns the shell-local settings (currently the global summon hotkey).
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn get_shell_settings(app: AppHandle) -> shell_settings::ShellSettings {
    shell_settings::load(&app)
}

/// Validates and persists the summon hotkey, then re-registers the global shortcut live.
///
/// The accelerator is parsed before anything is written so an invalid combination cannot brick
/// the next launch.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn set_shell_settings(
    app: AppHandle,
    hotkey: String,
) -> Result<shell_settings::ShellSettings, String> {
    let shortcut = hotkey
        .parse::<Shortcut>()
        .map_err(|error| format!("invalid hotkey: {error}"))?;
    let settings = shell_settings::ShellSettings { hotkey };
    shell_settings::save(&app, &settings).map_err(|error| error.to_string())?;
    apply_shortcut(&app, &shortcut)?;
    Ok(settings)
}

#[tauri::command]
async fn archive_settings() -> Result<ArchiveSettings, String> {
    let responses = request(request_envelope::Body::GetArchiveSettings(
        GetArchiveSettingsRequest {},
    ))
    .await?;
    for response in responses {
        match response.body {
            Some(response_envelope::Body::ArchiveSettings(settings)) => {
                return Ok(map_archive_settings(settings));
            }
            Some(response_envelope::Body::Error(error)) => return Err(error.message),
            _ => {}
        }
    }
    Err("daemon returned no archive settings".to_owned())
}

#[tauri::command]
async fn update_archive_settings(
    retention_days: Option<u32>,
    disk_budget_bytes: Option<u64>,
    excluded_applications: Vec<String>,
    excluded_titles: Vec<String>,
) -> Result<SettingsUpdateResult, String> {
    let responses = request(request_envelope::Body::UpdateArchiveSettings(
        UpdateArchiveSettingsRequest {
            retention_days,
            disk_budget_bytes,
            excluded_applications,
            excluded_titles,
        },
    ))
    .await?;
    for response in responses {
        match response.body {
            Some(response_envelope::Body::UpdateArchiveSettings(result)) => {
                let settings = result
                    .settings
                    .ok_or_else(|| "daemon returned empty archive settings".to_owned())?;
                return Ok(SettingsUpdateResult {
                    settings: map_archive_settings(settings),
                    captures_deleted: result.captures_deleted,
                    assets_scheduled: result.assets_scheduled,
                });
            }
            Some(response_envelope::Body::Error(error)) => return Err(error.message),
            _ => {}
        }
    }
    Err("daemon returned no settings update".to_owned())
}

#[tauri::command]
async fn delete_all_captures(confirmed: bool) -> Result<DeleteResult, String> {
    let responses = request(request_envelope::Body::DeleteCaptures(
        DeleteCapturesRequest {
            capture_ids: Vec::new(),
            before: String::new(),
            delete_all: true,
            confirmed,
        },
    ))
    .await?;
    for response in responses {
        match response.body {
            Some(response_envelope::Body::DeleteCaptures(result)) => {
                return Ok(DeleteResult {
                    captures_deleted: result.captures_deleted,
                    assets_scheduled: result.assets_scheduled,
                });
            }
            Some(response_envelope::Body::Error(error)) => return Err(error.message),
            _ => {}
        }
    }
    Err("daemon returned no deletion result".to_owned())
}

fn map_archive_settings(
    settings: screensearch_ipc::v1::ArchiveSettingsResponse,
) -> ArchiveSettings {
    ArchiveSettings {
        retention_days: settings.retention_days,
        disk_budget_bytes: settings.disk_budget_bytes,
        excluded_applications: settings.excluded_applications,
        excluded_titles: settings.excluded_titles,
        capture_count: settings.capture_count,
        asset_bytes: settings.asset_bytes,
    }
}

#[tauri::command]
async fn set_capture_paused(paused: bool) -> Result<bool, String> {
    let responses = request(request_envelope::Body::SetCapturePaused(
        SetCapturePausedRequest { paused },
    ))
    .await?;
    for response in responses {
        match response.body {
            Some(response_envelope::Body::SetCapturePaused(result)) => {
                return Ok(result.paused);
            }
            Some(response_envelope::Body::Error(error)) => return Err(error.message),
            _ => {}
        }
    }
    Err("daemon returned no capture state response".to_owned())
}

#[tauri::command]
async fn capture_once() -> Result<CaptureResult, String> {
    let responses = request(request_envelope::Body::Capture(CaptureRequest {})).await?;
    for response in responses {
        match response.body {
            Some(response_envelope::Body::Capture(capture)) => {
                return Ok(CaptureResult {
                    capture_id: capture.capture_id,
                    duplicate: capture.duplicate,
                    skipped_reason: capture.skipped_reason,
                });
            }
            Some(response_envelope::Body::Error(error)) => return Err(error.message),
            _ => {}
        }
    }
    Err("daemon returned no capture response".to_owned())
}

#[tauri::command]
async fn process_jobs(maximum: u32) -> Result<u32, String> {
    let responses = request(request_envelope::Body::ProcessJobs(ProcessJobsRequest {
        maximum,
    }))
    .await?;
    for response in responses {
        match response.body {
            Some(response_envelope::Body::ProcessJobs(result)) => return Ok(result.processed),
            Some(response_envelope::Body::Error(error)) => return Err(error.message),
            _ => {}
        }
    }
    Err("daemon returned no job response".to_owned())
}

#[tauri::command]
async fn capture_asset(capture_id: String) -> Result<CaptureAsset, String> {
    let responses = request(request_envelope::Body::GetCaptureAsset(
        GetCaptureAssetRequest { capture_id },
    ))
    .await?;
    for response in responses {
        match response.body {
            Some(response_envelope::Body::CaptureAsset(asset)) => {
                return Ok(CaptureAsset {
                    media_type: asset.media_type,
                    content: asset.content,
                });
            }
            Some(response_envelope::Body::Error(error)) => return Err(error.message),
            _ => {}
        }
    }
    Err("daemon returned no capture asset".to_owned())
}

#[tauri::command]
async fn search(
    query: String,
    generate_answer: bool,
    on_event: Channel<SearchUiEvent>,
) -> Result<(), String> {
    let client = IpcClient::new(DEFAULT_PIPE_NAME);
    let request = RequestEnvelope {
        request_id: uuid::Uuid::now_v7().to_string(),
        body: Some(request_envelope::Body::Search(SearchRequest {
            query,
            limit: 16,
            generate_answer,
        })),
    };
    client
        .request_each(request, |response| {
            match response.body {
                Some(response_envelope::Body::Search(event)) => match event.event {
                    Some(search_event::Event::Citation(citation)) => on_event
                        .send(SearchUiEvent::Citation {
                            capture_id: citation.capture_id,
                            chunk_id: citation.chunk_id,
                            excerpt: citation.excerpt,
                            score: citation.score,
                            captured_at: citation.captured_at,
                            application: citation.application,
                            window_title: citation.window_title,
                            width: citation.width,
                            height: citation.height,
                            bounds: citation
                                .bounds
                                .into_iter()
                                .map(|bounds| NormalizedRect {
                                    x: bounds.x,
                                    y: bounds.y,
                                    width: bounds.width,
                                    height: bounds.height,
                                })
                                .collect(),
                            match_kind: citation.match_kind,
                            ocr_model_id: citation.ocr_model_id,
                            embedding_model_id: citation.embedding_model_id,
                        })
                        .map_err(channel_error)?,
                    Some(search_event::Event::Token(token)) => on_event
                        .send(SearchUiEvent::Token { text: token.text })
                        .map_err(channel_error)?,
                    Some(search_event::Event::Completed(completed)) => on_event
                        .send(SearchUiEvent::Completed {
                            citation_count: completed.citation_count,
                        })
                        .map_err(channel_error)?,
                    None => {}
                },
                Some(response_envelope::Body::Error(error)) => {
                    return Err(IpcError::Handler(error.message));
                }
                _ => {}
            }
            Ok(())
        })
        .await
        .map_err(|error| error.to_string())
}

async fn request(
    body: request_envelope::Body,
) -> Result<Vec<screensearch_ipc::v1::ResponseEnvelope>, String> {
    IpcClient::new(DEFAULT_PIPE_NAME)
        .request(RequestEnvelope {
            request_id: uuid::Uuid::now_v7().to_string(),
            body: Some(body),
        })
        .await
        .map_err(|error| error.to_string())
}

#[allow(clippy::needless_pass_by_value)]
fn channel_error(error: tauri::Error) -> IpcError {
    IpcError::Handler(format!("Tauri channel: {error}"))
}

/// Live tray handles mutated by the background poller to reflect capture state.
struct TrayHandles {
    icon: TrayIcon<Wry>,
    status_item: MenuItem<Wry>,
    pause_item: MenuItem<Wry>,
}

/// Brings the main window to the foreground (used by the tray, hotkey, and menu).
fn summon_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Registers the global summon shortcut, clearing any previous binding first.
fn apply_shortcut(app: &AppHandle, shortcut: &Shortcut) -> Result<(), String> {
    let manager = app.global_shortcut();
    let _ = manager.unregister_all();
    manager
        .register(*shortcut)
        .map_err(|error| error.to_string())
}

/// Flips the daemon capture-pause state from the tray menu (best effort).
///
/// The tray poller and the in-window health poll both reflect the new state within a few seconds,
/// so no immediate menu mutation is needed here.
fn toggle_pause() {
    tauri::async_runtime::spawn(async move {
        if let Ok(status) = fetch_health().await {
            let _ = request(request_envelope::Body::SetCapturePaused(
                SetCapturePausedRequest {
                    paused: !status.capture_paused,
                },
            ))
            .await;
        }
    });
}

/// Polls daemon health every few seconds and reflects capture state in the tray.
fn spawn_health_poll(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let (status_line, pause_label) = match fetch_health().await {
                Ok(status) => {
                    let state = match status.capture_state.as_str() {
                        "paused" => "Paused",
                        "backpressured" => "Catching up",
                        _ => "Capturing",
                    };
                    let label = if status.capture_paused {
                        "Resume capture"
                    } else {
                        "Pause capture"
                    };
                    (format!("{state} · {} queued", status.queue_depth), label)
                }
                Err(_) => ("Daemon offline".to_owned(), "Pause capture"),
            };
            let tooltip = format!("ScreenSearch V2 — {status_line}");
            if let Some(handles) = app.try_state::<TrayHandles>() {
                let _ = handles.icon.set_tooltip(Some(tooltip.as_str()));
                let _ = handles.status_item.set_text(&status_line);
                let _ = handles.pause_item.set_text(pause_label);
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    });
}

/// Builds the tray icon, its menu, and the live status poller during application setup.
fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let handle = app.handle().clone();

    let open_item = MenuItem::with_id(app, "open", "Open ScreenSearch", true, None::<&str>)?;
    let status_item = MenuItem::with_id(app, "status", "Connecting…", false, None::<&str>)?;
    let pause_item = MenuItem::with_id(app, "pause", "Pause capture", true, None::<&str>)?;
    let separator_top = PredefinedMenuItem::separator(app)?;
    let separator_bottom = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit ScreenSearch", true, None::<&str>)?;
    let menu = MenuBuilder::new(app)
        .items(&[
            &open_item,
            &separator_top,
            &status_item,
            &pause_item,
            &separator_bottom,
            &quit_item,
        ])
        .build()?;

    let icon = app
        .default_window_icon()
        .expect("bundled window icon is configured")
        .clone();
    let tray = TrayIconBuilder::new()
        .icon(icon)
        .tooltip("ScreenSearch V2")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => summon_main_window(app),
            "pause" => toggle_pause(),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                summon_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    app.manage(TrayHandles {
        icon: tray,
        status_item,
        pause_item,
    });

    let settings = shell_settings::load(&handle);
    if let Ok(shortcut) = settings.hotkey.parse::<Shortcut>() {
        let _ = apply_shortcut(&handle, &shortcut);
    }

    spawn_health_poll(handle);
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        summon_main_window(app);
                        let _ = app.emit("summon-search", ());
                    }
                })
                .build(),
        )
        .setup(|app| {
            setup_tray(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Hide to tray instead of exiting; the daemon keeps running in its own process.
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            health,
            capture_once,
            process_jobs,
            capture_asset,
            set_capture_paused,
            archive_settings,
            update_archive_settings,
            delete_all_captures,
            search,
            get_shell_settings,
            set_shell_settings
        ])
        .run(tauri::generate_context!())
        .expect("run ScreenSearch V2 desktop application");
}
