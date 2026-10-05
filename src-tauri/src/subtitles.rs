//! Live subtitle capture and deduplicated translation without suspension.
use crate::{models::Game, runtime::Runtime};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SubtitleSettings {
    pub enabled: bool,
    pub interval_ms: u32,
    pub region_top: f32,
    pub region_height: f32,
    pub font_size: u32,
}
impl Default for SubtitleSettings {
    /// Keeps capture opt-in and selects the bottom dialogue region.
    fn default() -> Self {
        Self {
            enabled: false,
            interval_ms: 500,
            region_top: 0.62,
            region_height: 0.32,
            font_size: 30,
        }
    }
}
/// Cancels obsolete translations and hides captions whenever game focus changes.
pub fn start(app: AppHandle, runtime: Runtime) {
    tauri::async_runtime::spawn(async move {
        let mut last = String::new();
        let mut candidate = String::new();
        let mut stable = 0u8;
        let mut blanks = 0u8;
        let mut owner = 0u32;
        let mut pending: Option<tauri::async_runtime::JoinHandle<()>> = None;
        let generation = Arc::new(AtomicU64::new(0));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(u64, bool)>();
        loop {
            while let Ok((ticket, success)) = rx.try_recv() {
                if !success && ticket == generation.load(Ordering::SeqCst) {
                    last.clear();
                }
            }
            let (settings, game, paused, busy) = runtime.input_state();
            let cfg = settings.subtitles;
            let Some(game) = game.filter(|g| cfg.enabled && !paused && !busy && foreground(g.pid))
            else {
                generation.fetch_add(1, Ordering::SeqCst);
                if let Some(task) = pending.take() {
                    task.abort();
                }
                if let Some(w) = app.get_webview_window("subtitles") {
                    let _ = w.hide();
                }
                let _ = app.emit_to("subtitles", "live-caption", Option::<Caption>::None);
                last.clear();
                candidate.clear();
                stable = 0;
                blanks = 0;
                owner = 0;
                tokio::time::sleep(Duration::from_millis(300)).await;
                continue;
            };
            if owner != game.pid {
                if let Some(task) = pending.take() {
                    task.abort();
                }
                let _ = app.emit_to("subtitles", "live-caption", Option::<Caption>::None);
                last.clear();
                candidate.clear();
                stable = 0;
                owner = game.pid;
                generation.fetch_add(1, Ordering::SeqCst);
            }
            if let Some(w) = app.get_webview_window("subtitles") {
                let _ = w.set_position(tauri::PhysicalPosition::new(game.x, game.y));
                let _ = w.set_size(tauri::PhysicalSize::new(game.width, game.height));
                if !w.is_visible().unwrap_or(false) {
                    let _ = w.set_ignore_cursor_events(true);
                    let _ = w.set_focusable(false);
                    if let Ok(hwnd) = w.hwnd() {
                        unsafe {
                            windows_sys::Win32::UI::WindowsAndMessaging::SetWindowDisplayAffinity(
                                hwnd.0, 0x11,
                            );
                        }
                    }
                    let _ = w.show();
                }
            }
            let rt = runtime.clone();
            let captured_game = game.clone();
            let shot =
                tauri::async_runtime::spawn_blocking(move || capture_live(&rt, &captured_game))
                    .await;
            if let Ok(Ok(path)) = shot {
                let helper = runtime.executable.clone();
                let recognized = tauri::async_runtime::spawn_blocking(move || {
                    crate::ocr::recognize(&helper, &path)
                })
                .await;
                if let Ok(Ok(ocr)) = recognized {
                    let text = ocr
                        .words
                        .into_iter()
                        .map(|w| w.text)
                        .collect::<Vec<_>>()
                        .join(" ");
                    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    let key = text.to_lowercase();
                    if key.len() >= 5 {
                        blanks = 0;
                        if key == candidate {
                            stable = stable.saturating_add(1);
                        } else {
                            candidate = key.clone();
                            stable = 1;
                            generation.fetch_add(1, Ordering::SeqCst);
                            if let Some(task) = pending.take() {
                                task.abort();
                            }
                            let _ =
                                app.emit_to("subtitles", "live-caption", Option::<Caption>::None);
                        }
                        if stable >= 2 && key != last {
                            last = key;
                            let ticket = generation.load(Ordering::SeqCst);
                            let marker = generation.clone();
                            let rt = runtime.clone();
                            let handle = app.clone();
                            let font = cfg.font_size;
                            let sender = tx.clone();
                            pending = Some(tauri::async_runtime::spawn(async move {
                                let result =
                                    crate::context::fast(rt.clone(), text.clone(), text.clone())
                                        .await;
                                let success = result.is_ok();
                                if marker.load(Ordering::SeqCst) == ticket {
                                    match result {
                                        Ok(result) => {
                                            {
                                                let mut inner = rt.inner.lock().unwrap();
                                                if inner.history.last() != Some(&text) {
                                                    inner.history.push(text.clone());
                                                    if inner.history.len() > 12 {
                                                        inner.history.remove(0);
                                                    }
                                                }
                                            }
                                            let _ = handle.emit_to(
                                                "subtitles",
                                                "live-caption",
                                                Some(Caption {
                                                    source: text,
                                                    translation: result.translation,
                                                }),
                                            );
                                            let _ =
                                                handle.emit_to("subtitles", "caption-style", font);
                                        }
                                        Err(error) => {
                                            let _ = handle.emit("subtitle-error", error);
                                        }
                                    }
                                }
                                let _ = sender.send((ticket, success));
                            }));
                        }
                    } else {
                        blanks = blanks.saturating_add(1);
                        if blanks >= 2 {
                            generation.fetch_add(1, Ordering::SeqCst);
                            if let Some(task) = pending.take() {
                                task.abort();
                            }
                            last.clear();
                            candidate.clear();
                            stable = 0;
                            let _ =
                                app.emit_to("subtitles", "live-caption", Option::<Caption>::None);
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(
                cfg.interval_ms.clamp(250, 2000) as u64
            ))
            .await;
        }
    });
}
/// Verifies that foreground input belongs to the selected process.
fn foreground(pid: u32) -> bool {
    unsafe {
        let mut current = 0;
        windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(
            windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow(),
            &mut current,
        );
        current == pid
    }
}
/// Serializes frame acquisition with the interactive overlay and crops the dialogue region.
fn capture_live(runtime: &Runtime, game: &Game) -> Result<std::path::PathBuf, String> {
    let _guard = runtime.operation.lock().map_err(|e| e.to_string())?;
    if runtime.busy.load(Ordering::SeqCst) || !foreground(game.pid) {
        return Err("Игра не готова к захвату".into());
    }
    let current = runtime.inner.lock().unwrap();
    if current.game.as_ref().is_none_or(|g| g.pid != game.pid) || current.watchdog.is_some() {
        return Err("Захват приостановлен".into());
    }
    let cfg = current.data.settings.subtitles.clone();
    let mode = current.capture_mode.clone();
    drop(current);
    let directory = crate::native::directory(game.pid);
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    for name in ["frame.ready", "frame.bmp", "capture.request"] {
        let _ = fs::remove_file(directory.join(name));
    }
    if mode == "screen" {
        crate::native::call(
            &runtime.executable,
            &[
                "screen".into(),
                game.hwnd.to_string(),
                directory.join("frame.bmp").to_string_lossy().into_owned(),
            ],
        )?;
    } else {
        fs::write(directory.join("capture.request"), "1").map_err(|e| e.to_string())?;
        let deadline = std::time::Instant::now() + Duration::from_millis(800);
        loop {
            if let Ok(value) = fs::read_to_string(directory.join("frame.ready")) {
                if value != "ok" {
                    let _ = fs::remove_file(directory.join("capture.request"));
                    return Err(value);
                }
                break;
            }
            if std::time::Instant::now() > deadline {
                let _ = fs::remove_file(directory.join("capture.request"));
                return Err("Тайм-аут кадра субтитров".into());
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }
    let bitmap = image::open(directory.join("frame.bmp")).map_err(|e| e.to_string())?;
    let h = bitmap.height();
    let w = bitmap.width();
    let top = (h as f32 * cfg.region_top.clamp(0.35, 0.85)) as u32;
    let height = ((h as f32 * cfg.region_height.clamp(0.1, 0.45)) as u32)
        .min(h.saturating_sub(top))
        .max(1);
    let path = directory.join("live-subtitles.bmp");
    bitmap
        .crop_imm(0, top, w, height)
        .save_with_format(&path, image::ImageFormat::Bmp)
        .map_err(|e| e.to_string())?;
    Ok(path)
}
/// Carries the complete source and translated line to the transparent window.
#[derive(Clone, Serialize)]
pub struct Caption {
    pub source: String,
    pub translation: String,
}
