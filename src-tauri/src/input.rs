//! Edge-triggered keyboard and XInput bindings scoped to the selected game.
use crate::runtime::Runtime;
use std::{
    sync::atomic::Ordering,
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};
use windows_sys::Win32::UI::{
    Input::{
        KeyboardAndMouse::GetAsyncKeyState,
        XboxController::{XInputGetState, XINPUT_STATE},
    },
    WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow},
};

/// Returns whether the configured key or a connected controller binding is down.
fn pressed(hotkey: &str, gamepad: &str) -> bool {
    let key = crate::bindings::keyboard(hotkey);
    // SAFETY: Win32 input queries accept these fixed virtual keys and user indices.
    unsafe {
        if key.is_some_and(|(vk, modifiers)| {
            GetAsyncKeyState(vk) < 0
                && [0x11, 0x12, 0x10]
                    .iter()
                    .zip(modifiers)
                    .all(|(key, wanted)| (GetAsyncKeyState(*key) < 0) == wanted)
        }) {
            return true;
        }
        for index in 0..4 {
            let mut state: XINPUT_STATE = std::mem::zeroed();
            if XInputGetState(index, &mut state) == 0 {
                let Some((mask, lt, rt)) = crate::bindings::gamepad(gamepad) else {
                    continue;
                };
                if (mask != 0 || lt || rt)
                    && state.Gamepad.wButtons & mask == mask
                    && (!lt || state.Gamepad.bLeftTrigger > 160)
                    && (!rt || state.Gamepad.bRightTrigger > 160)
                {
                    return true;
                }
            }
        }
    }
    false
}
/// Polls only button edges; captures on game focus and resumes an existing pause.
pub fn start(app: AppHandle, runtime: Runtime) {
    thread::spawn(move || {
        let mut was_down = false;
        let mut previous = [false; 12];
        let mut repeat = Instant::now();
        loop {
            let (settings, game, paused, busy) = runtime.input_state();
            let navigation = controller_navigation();
            let escape = busy && unsafe { GetAsyncKeyState(0x1B) < 0 };
            let mut focused_pid = 0;
            unsafe {
                GetWindowThreadProcessId(GetForegroundWindow(), &mut focused_pid);
            }
            // Controller invoke bindings apply only over the game, so A/X/LB remain usable in the overlay.
            let pad_binding = if game.as_ref().is_some_and(|g| g.pid == focused_pid) {
                settings.gamepad.as_str()
            } else {
                "off"
            };
            let keyboard_binding = if game.as_ref().is_some_and(|g| g.pid == focused_pid) {
                settings.hotkey.as_str()
            } else {
                "off"
            };
            let down = pressed(keyboard_binding, pad_binding) || escape || (busy && navigation[6]);
            if down && !was_down {
                if let Some(game) = game.as_ref() {
                    let mut foreground_pid = 0;
                    // SAFETY: GetForegroundWindow is valid even with no focused window.
                    unsafe {
                        GetWindowThreadProcessId(GetForegroundWindow(), &mut foreground_pid);
                    }
                    if (paused || busy)
                        && (foreground_pid == game.pid || foreground_pid == std::process::id())
                    {
                        runtime.cancel_capture.store(true, Ordering::SeqCst);
                        if let Err(error) = runtime.resume_inner() {
                            let _ = app.emit("app-error", error);
                        }
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.set_always_on_top(false);
                            let _ = window.hide();
                        }
                        if let Some(window) = app.get_webview_window("overlay") {
                            let _ = window.hide();
                        }
                        unsafe {
                            SetForegroundWindow(game.hwnd as _);
                        }
                        let _ = app.emit("status-changed", ());
                    } else if !busy && foreground_pid == game.pid {
                        let rt = runtime.clone();
                        let handle = app.clone();
                        runtime.busy.store(true, Ordering::SeqCst);
                        thread::spawn(move || match rt.capture(&handle) {
                            Ok(frame) => {
                                let _ = handle.emit_to("main", "captured", frame);
                            }
                            Err(error) => {
                                let _ = handle.emit("app-error", error);
                            }
                        });
                    }
                }
            }
            if !down && focused_pid == std::process::id() {
                let target = if app
                    .get_webview_window("overlay")
                    .is_some_and(|w| w.is_visible().unwrap_or(false))
                {
                    "overlay"
                } else {
                    "main"
                };
                for (index, action) in [
                    "left",
                    "right",
                    "up",
                    "down",
                    "choose",
                    "save",
                    "resume",
                    "mode",
                    "previous-phrase",
                    "next-phrase",
                    "scroll-up",
                    "scroll-down",
                ]
                .iter()
                .enumerate()
                {
                    if navigation[index]
                        && (!previous[index]
                            || ((!(4..8).contains(&index))
                                && repeat.elapsed()
                                    > Duration::from_millis(if index >= 10 { 80 } else { 260 })))
                    {
                        let _ = app.emit_to(target, "controller-action", *action);
                        repeat = Instant::now();
                    }
                }
            }
            previous = navigation;
            was_down = down;
            thread::sleep(Duration::from_millis(16));
        }
    });
}
/// Reads standard XInput buttons and left-stick directions for foreground app navigation.
fn controller_navigation() -> [bool; 12] {
    for index in 0..4 {
        let mut state: XINPUT_STATE = unsafe { std::mem::zeroed() };
        if unsafe { XInputGetState(index, &mut state) } == 0 {
            let b = state.Gamepad.wButtons;
            return [
                b & 4 != 0 || state.Gamepad.sThumbLX < -16000,
                b & 8 != 0 || state.Gamepad.sThumbLX > 16000,
                b & 1 != 0 || state.Gamepad.sThumbLY > 16000,
                b & 2 != 0 || state.Gamepad.sThumbLY < -16000,
                b & 0x1000 != 0,
                b & 0x4000 != 0,
                b & 0x2000 != 0,
                b & 0x8000 != 0,
                b & 0x100 != 0,
                b & 0x200 != 0,
                state.Gamepad.bLeftTrigger > 160 || state.Gamepad.sThumbRY > 16000,
                state.Gamepad.bRightTrigger > 160 || state.Gamepad.sThumbRY < -16000,
            ];
        }
    }
    [false; 12]
}
