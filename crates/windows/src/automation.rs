#![allow(unsafe_code)]

use std::{ffi::c_void, mem::size_of, path::Path};

use async_trait::async_trait;
use screensearch_domain::{
    AutomationAction, AutomationFailureCode, AutomationKey, AutomationTarget, KeyModifier,
};
use screensearch_ports::{AutomationPlatform, PortError};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, HWND},
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoUninitialize,
            },
            RemoteDesktop::{
                WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, WTS_SESSIONSTATE_UNLOCK, WTSActive,
                WTSFreeMemory, WTSINFOEXW, WTSQuerySessionInformationW, WTSSessionInfoEx,
            },
            Threading::{
                OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                QueryFullProcessImageNameW,
            },
        },
        UI::{
            Accessibility::{
                CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationInvokePattern,
                IUIAutomationValuePattern, TreeScope_Descendants, UIA_InvokePatternId,
                UIA_ValuePatternId,
            },
            Input::KeyboardAndMouse::{
                INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
                KEYEVENTF_UNICODE, SendInput, VIRTUAL_KEY,
            },
            WindowsAndMessaging::{
                GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
            },
        },
    },
    core::{BSTR, PWSTR},
};

/// Production Windows adapter for exact foreground observations and typed action emission.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsAutomationPlatform;

#[async_trait]
impl AutomationPlatform for WindowsAutomationPlatform {
    async fn foreground_target(&self) -> Result<AutomationTarget, PortError> {
        tokio::task::spawn_blocking(read_foreground_target)
            .await
            .map_err(|error| PortError::Internal(format!("foreground task failed: {error}")))?
    }

    async fn session_is_unlocked(&self) -> Result<bool, PortError> {
        tokio::task::spawn_blocking(read_session_unlocked)
            .await
            .map_err(|error| PortError::Internal(format!("session task failed: {error}")))?
    }

    async fn execute_action(
        &self,
        target: &AutomationTarget,
        action: &AutomationAction,
    ) -> Result<(), PortError> {
        let target = target.clone();
        let action = action.clone();
        tokio::task::spawn_blocking(move || execute_native_action(&target, &action))
            .await
            .map_err(|error| PortError::Internal(format!("automation task failed: {error}")))?
    }
}

fn read_foreground_target() -> Result<AutomationTarget, PortError> {
    // SAFETY: Win32 getters are called with initialized output storage. Returned handles are
    // treated as borrowed except for the process handle, which is closed by `HandleGuard`.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return Err(PortError::Automation(AutomationFailureCode::TargetChanged));
        }
        let mut process_id = 0_u32;
        if GetWindowThreadProcessId(hwnd, Some(&raw mut process_id)) == 0 || process_id == 0 {
            return Err(PortError::Automation(AutomationFailureCode::TargetChanged));
        }
        let executable_name = executable_name(process_id)?;
        let title_length = usize::try_from(GetWindowTextLengthW(hwnd).max(0)).unwrap_or(0);
        let mut title_buffer = vec![0_u16; title_length.saturating_add(1).max(2)];
        let copied = usize::try_from(GetWindowTextW(hwnd, &mut title_buffer).max(0)).unwrap_or(0);
        let mut display_title = String::from_utf16_lossy(&title_buffer[..copied]);
        if display_title.trim().is_empty() {
            display_title.clone_from(&executable_name);
        }
        Ok(AutomationTarget {
            process_id,
            window_handle: hwnd.0 as usize as u64,
            executable_name,
            display_title,
        })
    }
}

unsafe fn executable_name(process_id: u32) -> Result<String, PortError> {
    // SAFETY: The PID comes from GetWindowThreadProcessId. The returned owned handle is closed
    // by HandleGuard, and QueryFullProcessImageNameW receives a writable bounded UTF-16 buffer.
    let handle = unsafe {
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id)
            .map_err(|_| PortError::Automation(AutomationFailureCode::TargetChanged))?
    };
    let _handle = HandleGuard(handle);
    let mut buffer = vec![0_u16; 32_768];
    let mut length = u32::try_from(buffer.len())
        .map_err(|_| PortError::Internal("process path buffer is too large".to_owned()))?;
    unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &raw mut length,
        )
        .map_err(|_| PortError::Automation(AutomationFailureCode::TargetChanged))?;
    }
    let path = String::from_utf16_lossy(
        &buffer[..usize::try_from(length)
            .map_err(|_| PortError::InvalidData("invalid process path length".to_owned()))?],
    );
    Path::new(&path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .ok_or(PortError::Automation(AutomationFailureCode::TargetChanged))
}

fn read_session_unlocked() -> Result<bool, PortError> {
    // SAFETY: WTS allocates the returned buffer. Its byte length is checked before casting, and
    // WTSFreeMemory releases it on every successful query path.
    unsafe {
        let mut buffer = PWSTR::null();
        let mut bytes = 0_u32;
        WTSQuerySessionInformationW(
            Some(WTS_CURRENT_SERVER_HANDLE),
            WTS_CURRENT_SESSION,
            WTSSessionInfoEx,
            &raw mut buffer,
            &raw mut bytes,
        )
        .map_err(|_| PortError::Automation(AutomationFailureCode::SessionLocked))?;
        let _buffer = WtsBuffer(buffer);
        if buffer.is_null() || usize::try_from(bytes).unwrap_or(0) < size_of::<WTSINFOEXW>() {
            return Err(PortError::Automation(AutomationFailureCode::SessionLocked));
        }
        #[allow(clippy::cast_ptr_alignment)]
        let info = &*(buffer.0.cast::<WTSINFOEXW>());
        if info.Level != 1 {
            return Err(PortError::Automation(AutomationFailureCode::SessionLocked));
        }
        let level = info.Data.WTSInfoExLevel1;
        Ok(level.SessionState == WTSActive
            && u32::try_from(level.SessionFlags).ok() == Some(WTS_SESSIONSTATE_UNLOCK))
    }
}

fn execute_native_action(
    target: &AutomationTarget,
    action: &AutomationAction,
) -> Result<(), PortError> {
    let foreground = read_foreground_target()?;
    if !target_identity_matches(&foreground, target) {
        return Err(PortError::Automation(AutomationFailureCode::TargetChanged));
    }
    match action {
        AutomationAction::UiaInvoke { automation_id } => execute_uia(target, automation_id, None),
        AutomationAction::UiaSetValue {
            automation_id,
            value,
        } => execute_uia(target, automation_id, Some(value)),
        AutomationAction::KeyChord { modifiers, key } => {
            send_keyboard_inputs(&encode_key_chord_inputs(modifiers, *key), modifiers)
        }
        AutomationAction::TypeText { text } => send_keyboard_inputs(&encode_text_inputs(text), &[]),
    }
}

fn execute_uia(
    target: &AutomationTarget,
    automation_id: &str,
    value: Option<&String>,
) -> Result<(), PortError> {
    let _com = ComApartment::initialize()?;
    // SAFETY: COM is initialized on this blocking thread. The target HWND was revalidated
    // immediately above, and every returned COM interface is managed by windows-rs.
    unsafe {
        let automation: IUIAutomation = CoCreateInstance(
            &CUIAutomation,
            None::<&windows::core::IUnknown>,
            CLSCTX_INPROC_SERVER,
        )
        .map_err(uia_internal)?;
        let hwnd_value = usize::try_from(target.window_handle)
            .map_err(|_| PortError::Automation(AutomationFailureCode::TargetChanged))?;
        let hwnd = HWND(hwnd_value as *mut c_void);
        let root = automation.ElementFromHandle(hwnd).map_err(uia_internal)?;
        let condition = automation.CreateTrueCondition().map_err(uia_internal)?;
        let elements = root
            .FindAll(TreeScope_Descendants, &condition)
            .map_err(uia_internal)?;
        let count = elements.Length().map_err(uia_internal)?;
        let mut match_element: Option<IUIAutomationElement> = None;
        for index in 0..count {
            let element = elements.GetElement(index).map_err(uia_internal)?;
            let current_id = element.CurrentAutomationId().map_err(uia_internal)?;
            if current_id == automation_id {
                if match_element.is_some() {
                    return Err(PortError::Automation(
                        AutomationFailureCode::ControlAmbiguous,
                    ));
                }
                match_element = Some(element);
            }
        }
        let element =
            match_element.ok_or(PortError::Automation(AutomationFailureCode::ControlMissing))?;
        if let Some(value) = value {
            let pattern: IUIAutomationValuePattern = element
                .GetCurrentPatternAs(UIA_ValuePatternId)
                .map_err(|_| PortError::Automation(AutomationFailureCode::ControlUnsupported))?;
            if pattern
                .CurrentIsReadOnly()
                .map_err(|_| PortError::Automation(AutomationFailureCode::ControlUnsupported))?
                .as_bool()
            {
                return Err(PortError::Automation(
                    AutomationFailureCode::ControlUnsupported,
                ));
            }
            pattern
                .SetValue(&BSTR::from(value.as_str()))
                .map_err(|_| PortError::Automation(AutomationFailureCode::InputBlocked))
        } else {
            let pattern: IUIAutomationInvokePattern = element
                .GetCurrentPatternAs(UIA_InvokePatternId)
                .map_err(|_| PortError::Automation(AutomationFailureCode::ControlUnsupported))?;
            pattern
                .Invoke()
                .map_err(|_| PortError::Automation(AutomationFailureCode::InputBlocked))
        }
    }
}

fn uia_internal(_: windows::core::Error) -> PortError {
    PortError::Automation(AutomationFailureCode::ControlUnsupported)
}

fn send_keyboard_inputs(inputs: &[INPUT], modifiers: &[KeyModifier]) -> Result<(), PortError> {
    // SAFETY: INPUT values are fully initialized keyboard records. SendInput receives the exact
    // element size required by Win32. On partial injection, modifier key-up records are sent
    // best-effort before returning the stable input_blocked failure.
    unsafe {
        let inserted = SendInput(
            inputs,
            i32::try_from(size_of::<INPUT>())
                .map_err(|_| PortError::Internal("INPUT size does not fit i32".to_owned()))?,
        );
        if validate_send_input_count(inputs.len(), inserted).is_err() {
            let releases = modifier_release_inputs(modifiers);
            if !releases.is_empty() {
                let _ = SendInput(
                    &releases,
                    i32::try_from(size_of::<INPUT>()).unwrap_or_default(),
                );
            }
            return Err(PortError::Automation(AutomationFailureCode::InputBlocked));
        }
    }
    Ok(())
}

pub(super) fn validate_send_input_count(expected: usize, actual: u32) -> Result<(), PortError> {
    if usize::try_from(actual).ok() == Some(expected) {
        Ok(())
    } else {
        Err(PortError::Automation(AutomationFailureCode::InputBlocked))
    }
}

pub(super) fn target_identity_matches(
    actual: &AutomationTarget,
    expected: &AutomationTarget,
) -> bool {
    actual.process_id == expected.process_id
        && actual.window_handle == expected.window_handle
        && actual
            .executable_name
            .eq_ignore_ascii_case(&expected.executable_name)
}

pub(super) fn encode_text_inputs(text: &str) -> Vec<INPUT> {
    text.encode_utf16()
        .flat_map(|code_unit| {
            [
                keyboard_input(0, code_unit, KEYEVENTF_UNICODE),
                keyboard_input(0, code_unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP),
            ]
        })
        .collect()
}

pub(super) fn encode_key_chord_inputs(modifiers: &[KeyModifier], key: AutomationKey) -> Vec<INPUT> {
    let mut inputs = Vec::with_capacity(modifiers.len().saturating_mul(2).saturating_add(2));
    for modifier in modifiers {
        inputs.push(keyboard_input(
            modifier_virtual_key(*modifier),
            0,
            KEYBD_EVENT_FLAGS::default(),
        ));
    }
    let key = automation_virtual_key(key);
    inputs.push(keyboard_input(key, 0, KEYBD_EVENT_FLAGS::default()));
    inputs.push(keyboard_input(key, 0, KEYEVENTF_KEYUP));
    inputs.extend(modifier_release_inputs(modifiers));
    inputs
}

fn modifier_release_inputs(modifiers: &[KeyModifier]) -> Vec<INPUT> {
    modifiers
        .iter()
        .rev()
        .map(|modifier| keyboard_input(modifier_virtual_key(*modifier), 0, KEYEVENTF_KEYUP))
        .collect()
}

fn keyboard_input(virtual_key: u16, scan_code: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(virtual_key),
                wScan: scan_code,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

#[cfg(test)]
pub(super) fn keyboard_event(input: &INPUT) -> KEYBDINPUT {
    // SAFETY: Every input returned by this module has INPUT_KEYBOARD type and initializes `ki`.
    unsafe { input.Anonymous.ki }
}

fn modifier_virtual_key(modifier: KeyModifier) -> u16 {
    match modifier {
        KeyModifier::Control => 0x11,
        KeyModifier::Alt => 0x12,
        KeyModifier::Shift => 0x10,
    }
}

#[allow(clippy::too_many_lines)]
fn automation_virtual_key(key: AutomationKey) -> u16 {
    match key {
        AutomationKey::A => 0x41,
        AutomationKey::B => 0x42,
        AutomationKey::C => 0x43,
        AutomationKey::D => 0x44,
        AutomationKey::E => 0x45,
        AutomationKey::F => 0x46,
        AutomationKey::G => 0x47,
        AutomationKey::H => 0x48,
        AutomationKey::I => 0x49,
        AutomationKey::J => 0x4A,
        AutomationKey::K => 0x4B,
        AutomationKey::L => 0x4C,
        AutomationKey::M => 0x4D,
        AutomationKey::N => 0x4E,
        AutomationKey::O => 0x4F,
        AutomationKey::P => 0x50,
        AutomationKey::Q => 0x51,
        AutomationKey::R => 0x52,
        AutomationKey::S => 0x53,
        AutomationKey::T => 0x54,
        AutomationKey::U => 0x55,
        AutomationKey::V => 0x56,
        AutomationKey::W => 0x57,
        AutomationKey::X => 0x58,
        AutomationKey::Y => 0x59,
        AutomationKey::Z => 0x5A,
        AutomationKey::Digit0 => 0x30,
        AutomationKey::Digit1 => 0x31,
        AutomationKey::Digit2 => 0x32,
        AutomationKey::Digit3 => 0x33,
        AutomationKey::Digit4 => 0x34,
        AutomationKey::Digit5 => 0x35,
        AutomationKey::Digit6 => 0x36,
        AutomationKey::Digit7 => 0x37,
        AutomationKey::Digit8 => 0x38,
        AutomationKey::Digit9 => 0x39,
        AutomationKey::Enter => 0x0D,
        AutomationKey::Escape => 0x1B,
        AutomationKey::Tab => 0x09,
        AutomationKey::Space => 0x20,
        AutomationKey::Backspace => 0x08,
        AutomationKey::Delete => 0x2E,
        AutomationKey::ArrowLeft => 0x25,
        AutomationKey::ArrowRight => 0x27,
        AutomationKey::ArrowUp => 0x26,
        AutomationKey::ArrowDown => 0x28,
        AutomationKey::Home => 0x24,
        AutomationKey::End => 0x23,
        AutomationKey::F1 => 0x70,
        AutomationKey::F2 => 0x71,
        AutomationKey::F3 => 0x72,
        AutomationKey::F4 => 0x73,
        AutomationKey::F5 => 0x74,
        AutomationKey::F6 => 0x75,
        AutomationKey::F7 => 0x76,
        AutomationKey::F8 => 0x77,
        AutomationKey::F9 => 0x78,
        AutomationKey::F10 => 0x79,
        AutomationKey::F11 => 0x7A,
        AutomationKey::F12 => 0x7B,
    }
}

struct HandleGuard(HANDLE);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        // SAFETY: The guard owns a successful OpenProcess handle and closes it exactly once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct WtsBuffer(PWSTR);

impl Drop for WtsBuffer {
    fn drop(&mut self) {
        // SAFETY: The pointer was allocated by WTSQuerySessionInformationW.
        unsafe {
            WTSFreeMemory(self.0.0.cast());
        }
    }
}

struct ComApartment;

impl ComApartment {
    fn initialize() -> Result<Self, PortError> {
        // SAFETY: Initializes COM for the current blocking thread; Drop balances successful calls.
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .map_err(|_| PortError::Automation(AutomationFailureCode::ControlUnsupported))?;
        }
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        // SAFETY: Balanced with the successful CoInitializeEx in `initialize`.
        unsafe {
            CoUninitialize();
        }
    }
}
