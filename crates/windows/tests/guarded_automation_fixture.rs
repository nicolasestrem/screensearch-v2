#![cfg(windows)]
//! Gated native fixture for guarded Windows automation.

#![allow(unsafe_code)]

use std::{
    ffi::c_void,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use screensearch_domain::{
    AutomationAction, AutomationFailureCode, AutomationKey, AutomationTarget, KeyModifier,
};
use screensearch_ports::{AutomationPlatform, PortError};
use screensearch_windows::WindowsAutomationPlatform;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::{
            LibraryLoader::GetModuleHandleW,
            Threading::{AttachThreadInput, GetCurrentThreadId},
        },
        UI::{
            Input::KeyboardAndMouse::SetFocus,
            WindowsAndMessaging::{
                ASFW_ANY, AllowSetForegroundWindow, BN_CLICKED, BS_PUSHBUTTON, BringWindowToTop,
                CREATESTRUCTW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
                DispatchMessageW, ES_AUTOHSCROLL, GWLP_USERDATA, GetDlgItem, GetDlgItemTextW,
                GetForegroundWindow, GetMessageW, GetWindowLongPtrW, GetWindowThreadProcessId,
                HMENU, HWND_NOTOPMOST, HWND_TOPMOST, LSFW_UNLOCK, LockSetForegroundWindow, MSG,
                PostQuitMessage, RegisterClassW, SW_RESTORE, SWP_NOMOVE, SWP_NOSIZE,
                SWP_SHOWWINDOW, SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
                SetWindowTextW, ShowWindow, SwitchToThisWindow, TranslateMessage, WINDOW_EX_STYLE,
                WINDOW_STYLE, WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_NCCREATE, WM_NCDESTROY,
                WNDCLASSW, WS_BORDER, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
            },
        },
    },
    core::PCWSTR,
};

const EDIT_ID: i32 = 101;
const BUTTON_ID: i32 = 102;
const FOCUS_EDIT_MESSAGE: u32 = WM_APP + 1;
static FIXTURE_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "sets foreground and emits native input; run with SCREENSEARCH_RUN_AUTOMATION_IT=1"]
async fn guarded_windows_automation_fixture_exercises_native_paths() {
    if std::env::var("SCREENSEARCH_RUN_AUTOMATION_IT").as_deref() != Ok("1") {
        eprintln!("set SCREENSEARCH_RUN_AUTOMATION_IT=1 to run the guarded automation fixture");
        return;
    }

    let primary = AutomationFixture::spawn("ScreenSearch guarded automation fixture");
    let platform = WindowsAutomationPlatform;
    primary.focus_edit();

    let target = wait_for_target(&platform, primary.hwnd).await;

    platform
        .execute_action(
            &target,
            &AutomationAction::UiaSetValue {
                automation_id: EDIT_ID.to_string(),
                value: "set by uia".to_owned(),
            },
        )
        .await
        .unwrap();
    wait_for(Duration::from_secs(2), || {
        primary.edit_text() == "set by uia"
    })
    .expect("UIA Value pattern did not update the fixture edit control");

    platform
        .execute_action(
            &target,
            &AutomationAction::UiaInvoke {
                automation_id: BUTTON_ID.to_string(),
            },
        )
        .await
        .unwrap();
    wait_for(Duration::from_secs(2), || {
        primary.button_invocations.load(Ordering::SeqCst) == 1
    })
    .expect("UIA Invoke pattern did not click the fixture button");

    primary.set_edit_text("");
    primary.focus_edit();
    platform
        .execute_action(
            &target,
            &AutomationAction::TypeText {
                text: "typed ✓".to_owned(),
            },
        )
        .await
        .unwrap();
    wait_for(Duration::from_secs(2), || primary.edit_text() == "typed ✓")
        .expect("UTF-16 SendInput text did not reach the fixture edit control");

    primary.set_edit_text("");
    primary.focus_edit();
    platform
        .execute_action(
            &target,
            &AutomationAction::KeyChord {
                modifiers: vec![KeyModifier::Control],
                key: AutomationKey::A,
            },
        )
        .await
        .unwrap();
    platform
        .execute_action(
            &target,
            &AutomationAction::TypeText {
                text: "replacement".to_owned(),
            },
        )
        .await
        .unwrap();
    wait_for(Duration::from_secs(2), || {
        primary.edit_text() == "replacement"
    })
    .expect("keyboard fallback chord did not select the fixture edit contents");

    let other = AutomationFixture::spawn("ScreenSearch guarded automation other fixture");
    other.focus_edit();
    let _other_target = wait_for_target(&platform, other.hwnd).await;
    let result = platform
        .execute_action(
            &target,
            &AutomationAction::TypeText {
                text: "must not type".to_owned(),
            },
        )
        .await;
    assert_eq!(
        result,
        Err(PortError::Automation(AutomationFailureCode::TargetChanged))
    );
    assert_eq!(primary.edit_text(), "replacement");
}

struct AutomationFixture {
    hwnd: usize,
    button_invocations: Arc<AtomicUsize>,
    thread: Option<thread::JoinHandle<()>>,
}

impl AutomationFixture {
    fn spawn(title: &'static str) -> Self {
        let (sender, receiver) = mpsc::channel();
        let button_invocations = Arc::new(AtomicUsize::new(0));
        let thread_invocations = Arc::clone(&button_invocations);
        let thread = thread::spawn(move || run_fixture_window(title, thread_invocations, sender));
        let hwnd = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("fixture window did not start");
        Self {
            hwnd,
            button_invocations,
            thread: Some(thread),
        }
    }

    fn focus_edit(&self) {
        focus_fixture_edit(self.hwnd);
    }

    fn edit_text(&self) -> String {
        // SAFETY: The control id is owned by this fixture. The bounded buffer is writable and
        // GetDlgItemTextW writes at most the provided capacity including the trailing NUL.
        unsafe {
            let mut buffer = vec![0_u16; 512];
            let copied = GetDlgItemTextW(hwnd_from_raw(self.hwnd), EDIT_ID, &mut buffer);
            String::from_utf16_lossy(&buffer[..usize::try_from(copied).unwrap_or(0)])
        }
    }

    fn set_edit_text(&self, value: &str) {
        let wide = wide_null(value);
        // SAFETY: The edit control belongs to the synthetic fixture window. Windows copies the
        // string during SetWindowTextW, so the temporary UTF-16 buffer is valid for the call.
        unsafe {
            let edit = GetDlgItem(Some(hwnd_from_raw(self.hwnd)), EDIT_ID)
                .expect("fixture edit control missing");
            SetWindowTextW(edit, PCWSTR(wide.as_ptr())).expect("set fixture edit text");
        }
    }
}

impl Drop for AutomationFixture {
    fn drop(&mut self) {
        // SAFETY: `WM_CLOSE` is posted only to the owned synthetic fixture window.
        unsafe {
            let _ = SendMessageW(hwnd_from_raw(self.hwnd), WM_CLOSE, None, None);
        }
        if let Some(thread) = self.thread.take() {
            thread.join().expect("fixture thread panicked");
        }
    }
}

fn focus_fixture_edit(raw_hwnd: usize) {
    // SAFETY: The HWND belongs to the synthetic fixture. The temporary thread-input attachment is
    // used only to let the test process foreground its own fixture window, then detached before
    // returning.
    unsafe {
        let hwnd = hwnd_from_raw(raw_hwnd);
        foreground_from_current_thread(hwnd);
        let _ = SendMessageW(hwnd, FOCUS_EDIT_MESSAGE, None, None);
    }
}

unsafe fn foreground_from_current_thread(hwnd: HWND) {
    // SAFETY: The HWND is the synthetic fixture window. This function temporarily attaches the
    // current thread to the foreground and target input queues for focus transfer only.
    unsafe {
        let current_thread = GetCurrentThreadId();
        let foreground_thread = {
            let foreground = GetForegroundWindow();
            if foreground.0.is_null() {
                0
            } else {
                GetWindowThreadProcessId(foreground, None)
            }
        };
        let target_thread = GetWindowThreadProcessId(hwnd, None);
        let foreground_attached = attach_thread_input(current_thread, foreground_thread, true);
        let target_attached = attach_thread_input(current_thread, target_thread, true);

        let _ = LockSetForegroundWindow(LSFW_UNLOCK);
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
        let _ = BringWindowToTop(hwnd);
        SwitchToThisWindow(hwnd, true);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_NOTOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );

        if target_attached {
            let _ = AttachThreadInput(current_thread, target_thread, false);
        }
        if foreground_attached {
            let _ = AttachThreadInput(current_thread, foreground_thread, false);
        }
    }
}

fn attach_thread_input(current_thread: u32, other_thread: u32, attach: bool) -> bool {
    if other_thread == 0 || other_thread == current_thread {
        return false;
    }
    // SAFETY: Attaches only the current test thread to a known GUI thread for the duration of
    // focus transfer, matching the Win32 AttachThreadInput contract.
    unsafe { AttachThreadInput(current_thread, other_thread, attach).as_bool() }
}

#[allow(clippy::needless_pass_by_value)]
fn run_fixture_window(
    title: &'static str,
    button_invocations: Arc<AtomicUsize>,
    sender: mpsc::Sender<usize>,
) {
    // SAFETY: The fixture registers a private window class, creates a parent window and two child
    // controls, and runs the standard message loop on the owning GUI thread.
    unsafe {
        let instance = GetModuleHandleW(None).expect("module handle").into();
        let class_name = wide_null(&format!(
            "ScreenSearchGuardedAutomationFixture{}{}",
            std::process::id(),
            FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let class = WNDCLASSW {
            lpfnWndProc: Some(fixture_wnd_proc),
            hInstance: instance,
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        assert_ne!(
            RegisterClassW(&raw const class),
            0,
            "register fixture window class"
        );

        let context = Box::new(FixtureContext { button_invocations });
        let context_ptr = Box::into_raw(context).cast::<c_void>();
        let title = wide_null(title);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            480,
            180,
            None,
            None,
            Some(instance),
            Some(context_ptr.cast_const()),
        )
        .expect("create fixture parent window");

        let edit_class = wide_null("EDIT");
        let button_class = wide_null("BUTTON");
        let empty = wide_null("");
        let button_text = wide_null("Invoke");
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(edit_class.as_ptr()),
            PCWSTR(empty.as_ptr()),
            WS_CHILD | WS_VISIBLE | WS_BORDER | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            18,
            22,
            420,
            28,
            Some(hwnd),
            Some(control_id(EDIT_ID)),
            Some(instance),
            None,
        )
        .expect("create fixture edit control");
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(button_class.as_ptr()),
            PCWSTR(button_text.as_ptr()),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_PUSHBUTTON as u32),
            18,
            64,
            120,
            32,
            Some(hwnd),
            Some(control_id(BUTTON_ID)),
            Some(instance),
            None,
        )
        .expect("create fixture button control");

        sender.send(hwnd.0 as usize).expect("send fixture hwnd");
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = SetForegroundWindow(hwnd);

        let mut message = MSG::default();
        while GetMessageW(&raw mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
}

unsafe extern "system" fn fixture_wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            // SAFETY: During WM_NCCREATE, lparam is a valid CREATESTRUCTW whose lpCreateParams is
            // the FixtureContext pointer passed to CreateWindowExW.
            unsafe {
                let create = &*(lparam.0 as *const CREATESTRUCTW);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            }
            LRESULT(1)
        }
        WM_COMMAND => {
            let control_id = i32::from(u16::try_from(wparam.0 & 0xFFFF).unwrap_or_default());
            let notification =
                u32::from(u16::try_from((wparam.0 >> 16) & 0xFFFF).unwrap_or_default());
            if control_id == BUTTON_ID && notification == BN_CLICKED {
                if let Some(context) = fixture_context(hwnd) {
                    context.button_invocations.fetch_add(1, Ordering::SeqCst);
                }
                return LRESULT(0);
            }
            // SAFETY: Delegates unhandled messages to the OS default window procedure.
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        FOCUS_EDIT_MESSAGE => {
            // SAFETY: The edit control is a child of this synthetic fixture window.
            unsafe {
                foreground_from_current_thread(hwnd);
                if let Ok(edit) = GetDlgItem(Some(hwnd), EDIT_ID) {
                    let _ = SetFocus(Some(edit));
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            // SAFETY: Destroys the owned fixture window in response to its close message.
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            // SAFETY: Ends the fixture thread's message loop after the owned window is destroyed.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        WM_NCDESTROY => {
            // SAFETY: Reclaims the FixtureContext that was boxed before CreateWindowExW.
            unsafe {
                let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
                if pointer != 0 {
                    let _ = Box::from_raw(pointer as *mut FixtureContext);
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
        }
        _ => {
            // SAFETY: Delegates unhandled messages to the OS default window procedure.
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
    }
}

fn fixture_context(hwnd: HWND) -> Option<&'static FixtureContext> {
    // SAFETY: GWLP_USERDATA contains a valid FixtureContext pointer between WM_NCCREATE and
    // WM_NCDESTROY. The returned reference is used only synchronously during message handling.
    unsafe {
        let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
        if pointer == 0 {
            None
        } else {
            Some(&*(pointer as *const FixtureContext))
        }
    }
}

struct FixtureContext {
    button_invocations: Arc<AtomicUsize>,
}

async fn wait_for_target(
    platform: &WindowsAutomationPlatform,
    expected_hwnd: usize,
) -> AutomationTarget {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        focus_fixture_edit(expected_hwnd);
        let observed = match platform.foreground_target().await {
            Ok(target)
                if target.window_handle == u64::try_from(expected_hwnd).unwrap_or_default() =>
            {
                return target;
            }
            Ok(target) => format!(
                "hwnd={} pid={} exe={} title={}",
                target.window_handle,
                target.process_id,
                target.executable_name,
                target.display_title
            ),
            Err(error) => format!("{error:?}"),
        };
        assert!(
            Instant::now() < deadline,
            "fixture window did not become foreground; last observed {observed}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn wait_for(timeout: Duration, mut condition: impl FnMut() -> bool) -> Result<(), ()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err(())
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

fn hwnd_from_raw(value: usize) -> HWND {
    HWND(value as *mut c_void)
}

fn control_id(value: i32) -> HMENU {
    let value = usize::try_from(value).expect("fixture control id must be positive");
    HMENU(value as *mut c_void)
}
