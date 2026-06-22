//! Tauri shell and typed proxy to the persistent ScreenSearch daemon.

mod shell_settings;

use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use screensearch_ipc::{
    IpcError,
    transport::{DEFAULT_PIPE_NAME, IpcClient},
    v1::{
        AbortAutomationRequest, ApproveAutomationRequest, AutomationAction as IpcAutomationAction,
        AutomationKey as IpcAutomationKey, AutomationKeyChord, AutomationKeyModifier,
        AutomationPlanV1 as IpcAutomationPlan, AutomationSafetyHeartbeatRequest,
        AutomationStatusRequest, AutomationTarget as IpcAutomationTarget, CaptureRequest,
        DeleteCapturesRequest, DeleteGenerationModelRequest, DownloadGenerationModelRequest,
        ExecuteAutomationRequest, GetArchiveSettingsRequest, GetAutomationForegroundTargetRequest,
        GetCaptureAssetRequest, HealthRequest, ImportLocalGenerationModelRequest,
        ListGenerationModelsRequest, ProcessJobsRequest, RequestEnvelope,
        ResetAutomationAbortRequest, SearchRequest, SelectGenerationModelRequest,
        SetAutomationEnabledRequest, SetCapturePausedRequest, TypeTextAction, UiaInvokeAction,
        UiaSetValueAction, UnloadGenerationModelRequest, UpdateArchiveSettingsRequest,
        automation_action, request_envelope, response_envelope, search_event,
    },
};
use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Emitter, Manager, WindowEvent, Wry,
    ipc::Channel,
    menu::{MenuBuilder, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tauri_plugin_notification::NotificationExt;

const ABORT_SHORTCUT: &str = "Ctrl+Alt+Shift+Esc";

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
#[serde(rename_all = "camelCase")]
struct GenerationModel {
    id: String,
    display_name: String,
    source: String,
    repository: String,
    filename: String,
    relative_path: String,
    content_hash: String,
    byte_length: u64,
    architecture: String,
    quantization: String,
    context_tokens: u32,
    supports_vision: bool,
    active: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AutomationTarget {
    process_id: u32,
    window_handle: u64,
    executable_name: String,
    display_title: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
enum AutomationAction {
    UiaInvoke {
        automation_id: String,
    },
    UiaSetValue {
        automation_id: String,
        value: String,
    },
    KeyChord {
        modifiers: Vec<String>,
        key: String,
    },
    TypeText {
        text: String,
    },
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AutomationPlan {
    target: AutomationTarget,
    actions: Vec<AutomationAction>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)]
struct AutomationStatus {
    enabled: bool,
    abort_available: bool,
    abort_active: bool,
    running: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AutomationApproval {
    approval_id: String,
    expires_at: String,
    action_count: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AutomationCommandError {
    code: String,
    message: String,
}

impl AutomationCommandError {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: "invalid_automation".to_owned(),
            message: message.into(),
        }
    }

    fn daemon(error: impl std::fmt::Display) -> Self {
        Self {
            code: "daemon_unavailable".to_owned(),
            message: error.to_string(),
        }
    }
}

#[derive(Clone, Serialize)]
// `rename_all` on a tagged enum renames the variant tags ("citation", …); the per-variant
// `rename_all_fields` renames the struct-variant fields so the UI receives camelCase keys.
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
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
        answer_status: String,
        answer_message: String,
    },
}

fn map_automation_plan(plan: AutomationPlan) -> Result<IpcAutomationPlan, AutomationCommandError> {
    Ok(IpcAutomationPlan {
        target: Some(IpcAutomationTarget {
            process_id: plan.target.process_id,
            window_handle: plan.target.window_handle,
            executable_name: plan.target.executable_name,
            display_title: plan.target.display_title,
        }),
        actions: plan
            .actions
            .into_iter()
            .map(map_automation_action)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn map_automation_action(
    action: AutomationAction,
) -> Result<IpcAutomationAction, AutomationCommandError> {
    let action = match action {
        AutomationAction::UiaInvoke { automation_id } => {
            automation_action::Action::UiaInvoke(UiaInvokeAction { automation_id })
        }
        AutomationAction::UiaSetValue {
            automation_id,
            value,
        } => automation_action::Action::UiaSetValue(UiaSetValueAction {
            automation_id,
            value,
        }),
        AutomationAction::KeyChord { modifiers, key } => {
            automation_action::Action::KeyChord(AutomationKeyChord {
                modifiers: modifiers
                    .into_iter()
                    .map(|modifier| map_automation_modifier(&modifier).map(i32::from))
                    .collect::<Result<Vec<_>, _>>()?,
                key: i32::from(map_automation_key(&key)?),
            })
        }
        AutomationAction::TypeText { text } => {
            automation_action::Action::TypeText(TypeTextAction { text })
        }
    };
    Ok(IpcAutomationAction {
        action: Some(action),
    })
}

fn map_automation_modifier(value: &str) -> Result<AutomationKeyModifier, AutomationCommandError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "control" => Ok(AutomationKeyModifier::Control),
        "alt" => Ok(AutomationKeyModifier::Alt),
        "shift" => Ok(AutomationKeyModifier::Shift),
        _ => Err(AutomationCommandError::invalid(
            "unknown automation key modifier",
        )),
    }
}

#[allow(clippy::too_many_lines)]
fn map_automation_key(value: &str) -> Result<IpcAutomationKey, AutomationCommandError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "a" => Ok(IpcAutomationKey::A),
        "b" => Ok(IpcAutomationKey::B),
        "c" => Ok(IpcAutomationKey::C),
        "d" => Ok(IpcAutomationKey::D),
        "e" => Ok(IpcAutomationKey::E),
        "f" => Ok(IpcAutomationKey::F),
        "g" => Ok(IpcAutomationKey::G),
        "h" => Ok(IpcAutomationKey::H),
        "i" => Ok(IpcAutomationKey::I),
        "j" => Ok(IpcAutomationKey::J),
        "k" => Ok(IpcAutomationKey::K),
        "l" => Ok(IpcAutomationKey::L),
        "m" => Ok(IpcAutomationKey::M),
        "n" => Ok(IpcAutomationKey::N),
        "o" => Ok(IpcAutomationKey::O),
        "p" => Ok(IpcAutomationKey::P),
        "q" => Ok(IpcAutomationKey::Q),
        "r" => Ok(IpcAutomationKey::R),
        "s" => Ok(IpcAutomationKey::S),
        "t" => Ok(IpcAutomationKey::T),
        "u" => Ok(IpcAutomationKey::U),
        "v" => Ok(IpcAutomationKey::V),
        "w" => Ok(IpcAutomationKey::W),
        "x" => Ok(IpcAutomationKey::X),
        "y" => Ok(IpcAutomationKey::Y),
        "z" => Ok(IpcAutomationKey::Z),
        "0" => Ok(IpcAutomationKey::Digit0),
        "1" => Ok(IpcAutomationKey::Digit1),
        "2" => Ok(IpcAutomationKey::Digit2),
        "3" => Ok(IpcAutomationKey::Digit3),
        "4" => Ok(IpcAutomationKey::Digit4),
        "5" => Ok(IpcAutomationKey::Digit5),
        "6" => Ok(IpcAutomationKey::Digit6),
        "7" => Ok(IpcAutomationKey::Digit7),
        "8" => Ok(IpcAutomationKey::Digit8),
        "9" => Ok(IpcAutomationKey::Digit9),
        "enter" => Ok(IpcAutomationKey::Enter),
        "escape" | "esc" => Ok(IpcAutomationKey::Escape),
        "tab" => Ok(IpcAutomationKey::Tab),
        "space" => Ok(IpcAutomationKey::Space),
        "backspace" => Ok(IpcAutomationKey::Backspace),
        "delete" => Ok(IpcAutomationKey::Delete),
        "arrowleft" => Ok(IpcAutomationKey::ArrowLeft),
        "arrowright" => Ok(IpcAutomationKey::ArrowRight),
        "arrowup" => Ok(IpcAutomationKey::ArrowUp),
        "arrowdown" => Ok(IpcAutomationKey::ArrowDown),
        "home" => Ok(IpcAutomationKey::Home),
        "end" => Ok(IpcAutomationKey::End),
        "f1" => Ok(IpcAutomationKey::F1),
        "f2" => Ok(IpcAutomationKey::F2),
        "f3" => Ok(IpcAutomationKey::F3),
        "f4" => Ok(IpcAutomationKey::F4),
        "f5" => Ok(IpcAutomationKey::F5),
        "f6" => Ok(IpcAutomationKey::F6),
        "f7" => Ok(IpcAutomationKey::F7),
        "f8" => Ok(IpcAutomationKey::F8),
        "f9" => Ok(IpcAutomationKey::F9),
        "f10" => Ok(IpcAutomationKey::F10),
        "f11" => Ok(IpcAutomationKey::F11),
        "f12" => Ok(IpcAutomationKey::F12),
        _ => Err(AutomationCommandError::invalid("unknown automation key")),
    }
}

fn automation_command_error(error: screensearch_ipc::v1::ErrorResponse) -> AutomationCommandError {
    AutomationCommandError {
        code: error.code,
        message: error.message,
    }
}

fn map_automation_status_response(
    status: screensearch_ipc::v1::AutomationStatusResponse,
) -> AutomationStatus {
    AutomationStatus {
        enabled: status.enabled,
        abort_available: status.abort_available,
        abort_active: status.abort_active,
        running: status.running,
    }
}

fn map_automation_target_response(target: IpcAutomationTarget) -> AutomationTarget {
    AutomationTarget {
        process_id: target.process_id,
        window_handle: target.window_handle,
        executable_name: target.executable_name,
        display_title: target.display_title,
    }
}

#[tauri::command]
async fn automation_status() -> Result<AutomationStatus, AutomationCommandError> {
    let responses = request(request_envelope::Body::AutomationStatus(
        AutomationStatusRequest {},
    ))
    .await
    .map_err(AutomationCommandError::daemon)?;
    for response in responses {
        match response.body {
            Some(response_envelope::Body::AutomationStatus(status)) => {
                return Ok(map_automation_status_response(status));
            }
            Some(response_envelope::Body::Error(error)) => {
                return Err(automation_command_error(error));
            }
            _ => {}
        }
    }
    Err(AutomationCommandError::daemon(
        "daemon returned no automation status",
    ))
}

#[tauri::command]
async fn set_automation_enabled(enabled: bool) -> Result<AutomationStatus, AutomationCommandError> {
    let responses = request(request_envelope::Body::SetAutomationEnabled(
        SetAutomationEnabledRequest { enabled },
    ))
    .await
    .map_err(AutomationCommandError::daemon)?;
    for response in responses {
        match response.body {
            Some(response_envelope::Body::SetAutomationEnabled(result)) => {
                let status = result.status.ok_or_else(|| {
                    AutomationCommandError::daemon("daemon returned empty automation status")
                })?;
                return Ok(map_automation_status_response(status));
            }
            Some(response_envelope::Body::Error(error)) => {
                return Err(automation_command_error(error));
            }
            _ => {}
        }
    }
    Err(AutomationCommandError::daemon(
        "daemon returned no automation enablement response",
    ))
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
async fn automation_foreground_target(
    app: AppHandle,
) -> Result<AutomationTarget, AutomationCommandError> {
    let window = app.get_webview_window("main");
    if let Some(window) = &window {
        let _ = window.hide();
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let result = async {
        let responses = request(request_envelope::Body::GetAutomationForegroundTarget(
            GetAutomationForegroundTargetRequest {},
        ))
        .await
        .map_err(AutomationCommandError::daemon)?;
        for response in responses {
            match response.body {
                Some(response_envelope::Body::AutomationForegroundTarget(result)) => {
                    let target = result.target.ok_or_else(|| {
                        AutomationCommandError::daemon("daemon returned empty automation target")
                    })?;
                    return Ok(map_automation_target_response(target));
                }
                Some(response_envelope::Body::Error(error)) => {
                    return Err(automation_command_error(error));
                }
                _ => {}
            }
        }
        Err(AutomationCommandError::daemon(
            "daemon returned no automation target",
        ))
    }
    .await;
    if let Some(window) = window {
        let _ = window.show();
        let _ = window.set_focus();
    }
    result
}

#[tauri::command]
async fn approve_automation(
    plan: AutomationPlan,
) -> Result<AutomationApproval, AutomationCommandError> {
    let responses = request(request_envelope::Body::ApproveAutomation(
        ApproveAutomationRequest {
            plan: Some(map_automation_plan(plan)?),
        },
    ))
    .await
    .map_err(AutomationCommandError::daemon)?;
    for response in responses {
        match response.body {
            Some(response_envelope::Body::ApproveAutomation(result)) => {
                return Ok(AutomationApproval {
                    approval_id: result.approval_id,
                    expires_at: result.expires_at,
                    action_count: result.action_count,
                });
            }
            Some(response_envelope::Body::Error(error)) => {
                return Err(automation_command_error(error));
            }
            _ => {}
        }
    }
    Err(AutomationCommandError::daemon(
        "daemon returned no automation approval",
    ))
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
async fn execute_automation(
    app: AppHandle,
    approval_id: String,
    plan: AutomationPlan,
) -> Result<String, AutomationCommandError> {
    let ipc_plan = map_automation_plan(plan)?;
    let window = app.get_webview_window("main");
    if let Some(window) = &window {
        let _ = window.hide();
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let result = async {
        let responses = request(request_envelope::Body::ExecuteAutomation(
            ExecuteAutomationRequest {
                approval_id,
                plan: Some(ipc_plan),
            },
        ))
        .await
        .map_err(AutomationCommandError::daemon)?;
        for response in responses {
            match response.body {
                Some(response_envelope::Body::ExecuteAutomation(result)) => {
                    return Ok(result.status);
                }
                Some(response_envelope::Body::Error(error)) => {
                    return Err(automation_command_error(error));
                }
                _ => {}
            }
        }
        Err(AutomationCommandError::daemon(
            "daemon returned no automation execution result",
        ))
    }
    .await;
    if let Some(window) = window {
        let _ = window.show();
        let _ = window.set_focus();
    }
    result
}

#[tauri::command]
async fn abort_automation() -> Result<bool, AutomationCommandError> {
    let responses = request(request_envelope::Body::AbortAutomation(
        AbortAutomationRequest {},
    ))
    .await
    .map_err(AutomationCommandError::daemon)?;
    for response in responses {
        match response.body {
            Some(response_envelope::Body::AbortAutomation(result)) => {
                return Ok(result.abort_active);
            }
            Some(response_envelope::Body::Error(error)) => {
                return Err(automation_command_error(error));
            }
            _ => {}
        }
    }
    Err(AutomationCommandError::daemon(
        "daemon returned no abort response",
    ))
}

#[tauri::command]
async fn reset_automation_abort() -> Result<bool, AutomationCommandError> {
    let responses = request(request_envelope::Body::ResetAutomationAbort(
        ResetAutomationAbortRequest {},
    ))
    .await
    .map_err(AutomationCommandError::daemon)?;
    for response in responses {
        match response.body {
            Some(response_envelope::Body::ResetAutomationAbort(result)) => {
                return Ok(result.abort_active);
            }
            Some(response_envelope::Body::Error(error)) => {
                return Err(automation_command_error(error));
            }
            _ => {}
        }
    }
    Err(AutomationCommandError::daemon(
        "daemon returned no abort reset response",
    ))
}

async fn send_automation_heartbeat(abort_registered: bool) -> Result<(), AutomationCommandError> {
    let responses = request(request_envelope::Body::AutomationSafetyHeartbeat(
        AutomationSafetyHeartbeatRequest { abort_registered },
    ))
    .await
    .map_err(AutomationCommandError::daemon)?;
    for response in responses {
        match response.body {
            Some(response_envelope::Body::AutomationSafetyHeartbeat(result)) if result.accepted => {
                return Ok(());
            }
            Some(response_envelope::Body::Error(error)) => {
                return Err(automation_command_error(error));
            }
            _ => {}
        }
    }
    Err(AutomationCommandError::daemon(
        "daemon returned no automation heartbeat response",
    ))
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

/// Registers the new summon hotkey live, then persists it on success.
///
/// The accelerator is parsed and re-registered before anything is written, so a rejected
/// combination is never saved and cannot leave the next launch without a working shortcut.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn set_shell_settings(
    app: AppHandle,
    hotkey: String,
) -> Result<shell_settings::ShellSettings, String> {
    let shortcut = hotkey
        .parse::<Shortcut>()
        .map_err(|error| format!("invalid hotkey: {error}"))?;
    // Register the shortcut first; only persist a hotkey the OS actually accepted, so a rejected
    // combination never leaves a dead hotkey in the settings file for the next launch.
    apply_shortcut(&app, shortcut)?;
    let settings = shell_settings::ShellSettings { hotkey };
    shell_settings::save(&app, &settings).map_err(|error| error.to_string())?;
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
async fn generation_models() -> Result<Vec<GenerationModel>, String> {
    let responses = request(request_envelope::Body::ListGenerationModels(
        ListGenerationModelsRequest {},
    ))
    .await?;
    for response in responses {
        match response.body {
            Some(response_envelope::Body::GenerationModels(result)) => {
                return Ok(result
                    .models
                    .into_iter()
                    .map(map_generation_model)
                    .collect());
            }
            Some(response_envelope::Body::Error(error)) => return Err(error.message),
            _ => {}
        }
    }
    Err("daemon returned no generation model list".to_owned())
}

#[tauri::command]
async fn import_local_generation_model(
    source_path: String,
    display_name: String,
    select: bool,
) -> Result<GenerationModel, String> {
    generation_model_command(request_envelope::Body::ImportLocalGenerationModel(
        ImportLocalGenerationModelRequest {
            source_path,
            display_name,
            select,
        },
    ))
    .await
}

#[tauri::command]
async fn download_generation_model(
    repository: String,
    filename: String,
    display_name: String,
    select: bool,
) -> Result<GenerationModel, String> {
    generation_model_command(request_envelope::Body::DownloadGenerationModel(
        DownloadGenerationModelRequest {
            repository,
            filename,
            display_name,
            select,
        },
    ))
    .await
}

#[tauri::command]
async fn select_generation_model(model_id: String) -> Result<GenerationModel, String> {
    generation_model_command(request_envelope::Body::SelectGenerationModel(
        SelectGenerationModelRequest { model_id },
    ))
    .await
}

#[tauri::command]
async fn delete_generation_model(model_id: String) -> Result<bool, String> {
    let responses = request(request_envelope::Body::DeleteGenerationModel(
        DeleteGenerationModelRequest { model_id },
    ))
    .await?;
    for response in responses {
        match response.body {
            Some(response_envelope::Body::DeleteGenerationModel(result)) => {
                return Ok(result.deleted);
            }
            Some(response_envelope::Body::Error(error)) => return Err(error.message),
            _ => {}
        }
    }
    Err("daemon returned no model deletion result".to_owned())
}

#[tauri::command]
async fn unload_generation_model() -> Result<bool, String> {
    let responses = request(request_envelope::Body::UnloadGenerationModel(
        UnloadGenerationModelRequest {},
    ))
    .await?;
    for response in responses {
        match response.body {
            Some(response_envelope::Body::UnloadGenerationModel(result)) => {
                return Ok(result.unloaded);
            }
            Some(response_envelope::Body::Error(error)) => return Err(error.message),
            _ => {}
        }
    }
    Err("daemon returned no model unload result".to_owned())
}

async fn generation_model_command(body: request_envelope::Body) -> Result<GenerationModel, String> {
    let responses = request(body).await?;
    for response in responses {
        match response.body {
            Some(response_envelope::Body::GenerationModel(result)) => {
                let model = result
                    .model
                    .ok_or_else(|| "daemon returned an empty generation model".to_owned())?;
                return Ok(map_generation_model(model));
            }
            Some(response_envelope::Body::Error(error)) => return Err(error.message),
            _ => {}
        }
    }
    Err("daemon returned no generation model".to_owned())
}

fn map_generation_model(model: screensearch_ipc::v1::GenerationModel) -> GenerationModel {
    GenerationModel {
        id: model.id,
        display_name: model.display_name,
        source: model.source,
        repository: model.repository,
        filename: model.filename,
        relative_path: model.relative_path,
        content_hash: model.content_hash,
        byte_length: model.byte_length,
        architecture: model.architecture,
        quantization: model.quantization,
        context_tokens: model.context_tokens,
        supports_vision: model.supports_vision,
        active: model.active,
    }
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
                            answer_status: completed.answer_status,
                            answer_message: completed.answer_message,
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
    /// Last known daemon pause state; the tray toggle flips this optimistically to avoid a
    /// read-then-write race on rapid clicks.
    paused: AtomicBool,
}

/// The currently registered global summon shortcut, so a change unregisters only the old binding.
struct ActiveShortcut(Mutex<Option<Shortcut>>);

/// Fixed emergency-abort registration state reported to the daemon heartbeat.
struct AbortShortcutState {
    shortcut: Shortcut,
    registered: AtomicBool,
}

/// Brings the main window to the foreground (used by the tray, hotkey, and menu).
fn summon_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Registers the global summon shortcut, replacing only the previously registered binding.
///
/// On failure the previous binding is restored, so a rejected hotkey never leaves the user with
/// no working summon shortcut.
fn apply_shortcut(app: &AppHandle, shortcut: Shortcut) -> Result<(), String> {
    let manager = app.global_shortcut();
    let Some(state) = app.try_state::<ActiveShortcut>() else {
        return manager
            .register(shortcut)
            .map_err(|error| error.to_string());
    };
    let mut active = state.0.lock().expect("active shortcut lock poisoned");
    if let Some(previous) = *active {
        let _ = manager.unregister(previous);
    }
    match manager.register(shortcut) {
        Ok(()) => {
            *active = Some(shortcut);
            Ok(())
        }
        Err(error) => {
            if let Some(previous) = *active {
                let _ = manager.register(previous);
            }
            Err(error.to_string())
        }
    }
}

/// Flips the daemon capture-pause state from the tray menu, then notifies and refreshes the tray.
///
/// The target state is derived by atomically flipping the last known state rather than reading it
/// first, so two rapid clicks issue opposite requests instead of racing on a stale read.
fn toggle_pause(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let target = {
            let Some(handles) = app.try_state::<TrayHandles>() else {
                return;
            };
            !handles.paused.fetch_xor(true, Ordering::SeqCst)
        };
        if request(request_envelope::Body::SetCapturePaused(
            SetCapturePausedRequest { paused: target },
        ))
        .await
        .is_ok()
        {
            notify_capture_state(&app, target);
            refresh_tray(&app).await;
        } else if let Some(handles) = app.try_state::<TrayHandles>() {
            // Roll back the optimistic flip if the daemon never applied it.
            handles.paused.store(!target, Ordering::SeqCst);
        }
    });
}

/// Shows a native notification when capture is paused or resumed.
///
/// Best effort: Windows toast notifications require a registered application id, so they appear in
/// packaged builds but may be suppressed in `tauri dev`.
fn notify_capture_state(app: &AppHandle, paused: bool) {
    let (title, body) = if paused {
        ("ScreenSearch paused", "Screen capture is paused.")
    } else {
        ("ScreenSearch resumed", "Screen capture is active.")
    };
    let _ = app.notification().builder().title(title).body(body).show();
}

/// Reflects current daemon capture state in the tray tooltip, status line, and pause label.
async fn refresh_tray(app: &AppHandle) {
    let result = fetch_health().await;
    let (status_line, pause_label) = match &result {
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
        if let Ok(status) = &result {
            handles
                .paused
                .store(status.capture_paused, Ordering::SeqCst);
        }
    }
}

/// Polls daemon health every few seconds and reflects capture state in the tray.
fn spawn_health_poll(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            refresh_tray(&app).await;
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    });
}

/// Keeps the daemon informed that the shell still owns the fixed emergency-abort shortcut.
fn spawn_automation_heartbeat(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let registered = app
                .try_state::<AbortShortcutState>()
                .is_some_and(|state| state.registered.load(Ordering::SeqCst));
            let _ = send_automation_heartbeat(registered).await;
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
            "pause" => toggle_pause(app),
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
        paused: AtomicBool::new(false),
    });
    app.manage(ActiveShortcut(Mutex::new(None)));

    let settings = shell_settings::load(&handle);
    if let Ok(shortcut) = settings.hotkey.parse::<Shortcut>() {
        let _ = apply_shortcut(&handle, shortcut);
    }

    let abort_shortcut = ABORT_SHORTCUT
        .parse::<Shortcut>()
        .expect("fixed guarded automation abort shortcut is valid");
    let abort_registered = handle.global_shortcut().register(abort_shortcut).is_ok();
    app.manage(AbortShortcutState {
        shortcut: abort_shortcut,
        registered: AtomicBool::new(abort_registered),
    });

    spawn_health_poll(handle);
    spawn_automation_heartbeat(app.handle().clone());
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        if app
                            .try_state::<AbortShortcutState>()
                            .is_some_and(|state| state.shortcut == *shortcut)
                        {
                            tauri::async_runtime::spawn(async {
                                let _ = abort_automation().await;
                            });
                            return;
                        }
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
            generation_models,
            import_local_generation_model,
            download_generation_model,
            select_generation_model,
            delete_generation_model,
            unload_generation_model,
            search,
            get_shell_settings,
            set_shell_settings,
            automation_status,
            set_automation_enabled,
            automation_foreground_target,
            approve_automation,
            execute_automation,
            abort_automation,
            reset_automation_abort
        ])
        .run(tauri::generate_context!())
        .expect("run ScreenSearch V2 desktop application");
}

#[cfg(test)]
mod tests {
    use screensearch_ipc::v1::automation_action;

    use super::{
        ABORT_SHORTCUT, AutomationAction, AutomationPlan, AutomationTarget, NormalizedRect,
        SearchUiEvent, map_automation_plan,
    };

    #[test]
    fn citation_event_serializes_with_camel_case_fields() {
        let event = SearchUiEvent::Citation {
            capture_id: "cap".to_owned(),
            chunk_id: "chunk".to_owned(),
            excerpt: "text".to_owned(),
            score: 0.5,
            captured_at: "2026-06-21T15:43:13.785+00:00".to_owned(),
            application: "App".to_owned(),
            window_title: "Title".to_owned(),
            width: 100,
            height: 50,
            bounds: vec![NormalizedRect {
                x: 0.1,
                y: 0.2,
                width: 0.3,
                height: 0.4,
            }],
            match_kind: "hybrid".to_owned(),
            ocr_model_id: "ocr".to_owned(),
            embedding_model_id: "embed".to_owned(),
        };
        let value = serde_json::to_value(&event).expect("serialize citation event");
        assert_eq!(value["kind"], "citation");
        assert_eq!(value["captureId"], "cap");
        assert_eq!(value["chunkId"], "chunk");
        assert_eq!(value["capturedAt"], "2026-06-21T15:43:13.785+00:00");
        assert_eq!(value["windowTitle"], "Title");
        assert_eq!(value["matchKind"], "hybrid");
        assert_eq!(value["ocrModelId"], "ocr");
        assert_eq!(value["embeddingModelId"], "embed");
        // The UI reads camelCase, so snake_case keys must never be emitted.
        assert!(value.get("captured_at").is_none());
        assert!(value.get("chunk_id").is_none());
        assert!(value.get("capture_id").is_none());
    }

    #[test]
    fn completed_event_serializes_with_camel_case_fields() {
        let value = serde_json::to_value(SearchUiEvent::Completed {
            citation_count: 3,
            answer_status: "answered".to_owned(),
            answer_message: String::new(),
        })
        .expect("serialize completed event");
        assert_eq!(value["kind"], "completed");
        assert_eq!(value["citationCount"], 3);
        assert_eq!(value["answerStatus"], "answered");
        assert!(value.get("citation_count").is_none());
    }

    #[test]
    fn automation_plan_maps_to_typed_protobuf_actions() {
        let plan = map_automation_plan(AutomationPlan {
            target: AutomationTarget {
                process_id: 42,
                window_handle: 9001,
                executable_name: "fixture.exe".to_owned(),
                display_title: "Fixture".to_owned(),
            },
            actions: vec![
                AutomationAction::KeyChord {
                    modifiers: vec!["control".to_owned(), "shift".to_owned()],
                    key: "s".to_owned(),
                },
                AutomationAction::TypeText {
                    text: "hello".to_owned(),
                },
            ],
        })
        .unwrap();
        assert_eq!(plan.actions.len(), 2);
        let Some(automation_action::Action::KeyChord(chord)) = &plan.actions[0].action else {
            panic!("expected key chord");
        };
        assert_eq!(chord.key, 19);
    }

    #[test]
    fn fixed_abort_shortcut_is_parseable() {
        assert!(
            ABORT_SHORTCUT
                .parse::<tauri_plugin_global_shortcut::Shortcut>()
                .is_ok()
        );
    }
}
