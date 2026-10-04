//! Serialized attachment, frame acquisition, OCR, pause, and automatic recovery.
use crate::llm::{self, Llm};
use crate::{context, dictionary::Dictionary, mini::Mini, models::*, native};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    process::Child,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};

pub struct Inner {
    pub data: SavedData,
    pub game: Option<Game>,
    pub watchdog: Option<Child>,
    pub history: Vec<String>,
    pub last_frame: Option<Frame>,
    pub capture_mode: String,
}
#[derive(Clone)]
pub struct Runtime {
    pub inner: Arc<Mutex<Inner>>,
    pub operation: Arc<Mutex<()>>,
    pub busy: Arc<AtomicBool>,
    pub cancel_capture: Arc<AtomicBool>,
    pub executable: PathBuf,
    pub dll: PathBuf,
    pub store: PathBuf,
    pub dictionary: Arc<Dictionary>,
    pub mini: Mini,
    pub llm: Llm,
    pub translation_cache: Arc<Mutex<HashMap<String, ContextTranslation>>>,
}
impl Runtime {
    /// Creates state from disk. Corrupt stores fail visibly instead of being overwritten.
    pub fn new(
        executable: PathBuf,
        dll: PathBuf,
        store: PathBuf,
        dictionary_path: PathBuf,
    ) -> Result<Self, String> {
        let model_root = store
            .parent()
            .ok_or("Нет каталога приложения")?
            .join("models-v2/opus");
        let data = if store.exists() {
            serde_json::from_slice(&fs::read(&store).map_err(|e| e.to_string())?)
                .map_err(|e| format!("Ошибка словаря {}: {e}", store.display()))?
        } else {
            SavedData::default()
        };
        let dictionary =
            serde_json::from_slice(&fs::read(dictionary_path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let llm_root = store
            .parent()
            .ok_or("Нет каталога приложения")?
            .join("models-v2");
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                data,
                game: None,
                watchdog: None,
                history: vec![],
                last_frame: None,
                capture_mode: "hook".into(),
            })),
            operation: Arc::new(Mutex::new(())),
            busy: Arc::new(AtomicBool::new(false)),
            cancel_capture: Arc::new(AtomicBool::new(false)),
            executable,
            dll,
            store,
            dictionary: Arc::new(dictionary),
            mini: Mini::new(model_root),
            llm: Llm::new(llm_root),
            translation_cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }
    /// Builds current UI state, checking whether the watchdog has already resumed.
    pub fn status(&self) -> Status {
        let mut inner = self.inner.lock().unwrap();
        if let Some(child) = inner.watchdog.as_mut() {
            if matches!(child.try_wait(), Ok(Some(_))) {
                inner.watchdog = None;
            }
        }
        Status {
            game: inner.game.clone(),
            paused: inner.watchdog.is_some(),
            busy: self.busy.load(Ordering::SeqCst),
            settings: inner.data.settings.clone(),
            entries: inner.data.entries.clone(),
            dictionary_size: self.dictionary.len(),
            capture_mode: inner.capture_mode.clone(),
        }
    }
    /// Reads only input bindings and session state; avoids cloning vocabulary on every poll.
    pub fn input_state(&self) -> (Settings, Option<Game>, bool, bool) {
        let mut inner = self.inner.lock().unwrap();
        if inner
            .watchdog
            .as_mut()
            .is_some_and(|c| matches!(c.try_wait(), Ok(Some(_))))
        {
            inner.watchdog = None;
        }
        (
            inner.data.settings.clone(),
            inner.game.clone(),
            inner.watchdog.is_some(),
            self.busy.load(Ordering::SeqCst),
        )
    }
    /// Signals the watchdog and waits for it to restore the game before returning.
    pub fn resume_inner(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        if inner.watchdog.is_none() {
            return Ok(());
        }
        let pid = inner.game.as_ref().ok_or("Нет выбранной игры")?.pid;
        native::call(
            &self.executable,
            &[
                "resume".into(),
                pid.to_string(),
                std::process::id().to_string(),
            ],
        )?;
        if let Some(mut child) = inner.watchdog.take() {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if child.try_wait().map_err(|e| e.to_string())?.is_some() {
                    break;
                }
                if Instant::now() > deadline {
                    inner.watchdog = Some(child);
                    return Err("Сторож паузы ещё не подтвердил возобновление".into());
                }
                thread::sleep(Duration::from_millis(20));
            }
        }
        Ok(())
    }
    /// Reads a requested frame before suspension and performs local Windows OCR.
    /// Any capture/OCR error restores both the game and application window.
    pub fn capture(&self, app: &AppHandle) -> Result<Frame, String> {
        let _guard = self.operation.lock().unwrap();
        self.busy.store(true, Ordering::SeqCst);
        self.cancel_capture.store(false, Ordering::SeqCst);
        let _ = app.emit("capture-progress", "Получаю кадр игры…");
        let initial = {
            let inner = self.inner.lock().unwrap();
            inner.game.clone().filter(|_| inner.data.settings.overlay)
        };
        // Paint feedback immediately, keeping game focus until its back buffer has been read.
        if let Some(game) = initial {
            let _ = self.show_loading(app, &game, false);
        }
        let result = self.capture_inner(app);
        if result.is_err() {
            let _ = self.resume_inner();
        }
        self.busy.store(false, Ordering::SeqCst);
        if self.cancel_capture.load(Ordering::SeqCst) {
            let _ = app.emit("status-changed", ());
            return Err("Захват отменён".into());
        }
        if let Ok(frame) = &result {
            self.inner.lock().unwrap().last_frame = Some(frame.clone());
            if self.inner.lock().unwrap().data.settings.overlay {
                if let Err(error) = self.show_overlay(app) {
                    let _ = app.emit("app-error", error);
                }
            } else if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        } else if let Some(window) = app.get_webview_window("main") {
            if let Some(overlay) = app.get_webview_window("overlay") {
                let _ = overlay.hide();
            }
            let _ = window.show();
            let _ = window.set_focus();
        }
        let _ = app.emit("status-changed", ());
        result
    }
    /// Implements the capture transaction while the operation mutex is held.
    fn capture_inner(&self, app: &AppHandle) -> Result<Frame, String> {
        let (mut game, settings, mode) = {
            let inner = self.inner.lock().unwrap();
            (
                inner.game.clone().ok_or("Сначала подключите игру")?,
                inner.data.settings.clone(),
                inner.capture_mode.clone(),
            )
        };
        self.resume_inner()?;
        // Validate PID and window again before touching a previously selected process.
        let refreshed: Game = serde_json::from_str(&native::call(
            &self.executable,
            &["window".into(), game.hwnd.to_string()],
        )?)
        .map_err(|e| e.to_string())?;
        if refreshed.pid != game.pid || refreshed.path != game.path || refreshed.hwnd != game.hwnd {
            return Err("Игра закрыта. Подключите новый процесс.".into());
        }
        game = refreshed;
        self.inner.lock().unwrap().game = Some(game.clone());
        let already_focused = unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() == game.hwnd as _
        };
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.hide();
        }
        if mode == "screen" {
            if let Some(window) = app.get_webview_window("overlay") {
                let _ = window.hide();
            }
        }
        // SAFETY: HWND was returned and revalidated by native window enumeration.
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow(game.hwnd as _);
        }
        if !already_focused {
            thread::sleep(Duration::from_millis(100));
        }
        let dir = native::directory(game.pid);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for name in ["frame.ready", "frame.bmp", "capture.request"] {
            let _ = fs::remove_file(dir.join(name));
        }
        if mode == "screen" {
            native::call(
                &self.executable,
                &[
                    "screen".into(),
                    game.hwnd.to_string(),
                    dir.join("frame.bmp").to_string_lossy().into_owned(),
                ],
            )?;
            fs::write(dir.join("frame.ready"), "ok").map_err(|e| e.to_string())?;
        } else {
            fs::write(dir.join("capture.request"), "1").map_err(|e| e.to_string())?;
        }
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            if self.cancel_capture.load(Ordering::SeqCst) {
                let _ = fs::remove_file(dir.join("capture.request"));
                return Err("Захват отменён".into());
            }
            if let Ok(status) = fs::read_to_string(dir.join("frame.ready")) {
                if status != "ok" {
                    return Err(status);
                }
                break;
            }
            if Instant::now() > deadline {
                let _ = fs::remove_file(dir.join("capture.request"));
                return Err(
                    "DLL не получила кадр. Игра должна рендерить в DirectX 11 и быть развёрнута."
                        .into(),
                );
            }
            thread::sleep(Duration::from_millis(25));
        }
        let mut warning = None;
        if settings.auto_pause {
            if self.cancel_capture.load(Ordering::SeqCst) {
                return Err("Захват отменён".into());
            }
            let _ = fs::remove_file(dir.join("paused.status"));
            let mut child = native::watchdog(&self.executable, game.pid, mode == "hook")?;
            let deadline = Instant::now() + Duration::from_secs(4);
            loop {
                if fs::read_to_string(dir.join("paused.status"))
                    .ok()
                    .as_deref()
                    == Some(&child.id().to_string())
                {
                    self.inner.lock().unwrap().watchdog = Some(child);
                    break;
                }
                if child.try_wait().map_err(|e| e.to_string())?.is_some() {
                    return Err("Не удалось приостановить игру".into());
                }
                if Instant::now() > deadline {
                    // Keep ownership so rollback can signal the already created watchdog.
                    self.inner.lock().unwrap().watchdog = Some(child);
                    return Err("Истекло время ожидания паузы".into());
                }
                thread::sleep(Duration::from_millis(20));
            }
        }
        if settings.overlay {
            self.show_loading(app, &game, true)?;
        }
        let _ = app.emit("capture-progress", "Распознаю слова…");
        let executable = self.executable.clone();
        let ocr_path = dir.join("frame.bmp");
        let ocr_job = thread::spawn(move || crate::ocr::recognize(&executable, &ocr_path));
        let image = fs::read(dir.join("frame.bmp")).map_err(|e| e.to_string())?;
        let bitmap = image::load_from_memory_with_format(&image, image::ImageFormat::Bmp)
            .map_err(|e| e.to_string())?;
        let mut picture = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut picture, 92)
            .encode_image(&bitmap.to_rgb8())
            .map_err(|e| e.to_string())?;
        let image = format!("data:image/jpeg;base64,{}", STANDARD.encode(picture));
        let _ = app.emit_to("overlay", "frame-image", &image);
        let ocr = match ocr_job.join().map_err(|_| "Сбой OCR потока")? {
            Ok(ocr) => ocr,
            Err(error) => {
                warning = Some(error);
                Ocr {
                    width: bitmap.width(),
                    height: bitmap.height(),
                    text: String::new(),
                    words: vec![],
                }
            }
        };
        let blocks = context::segment(&ocr.words, &self.dictionary);
        let mut inner = self.inner.lock().unwrap();
        let mut dialogue = String::new();
        let mut previous_line = None;
        for word in ocr.words.iter().filter(|w| w.y > ocr.height as f64 * 0.45) {
            if !dialogue.is_empty() {
                dialogue.push(if previous_line == Some(word.line) {
                    ' '
                } else {
                    '\n'
                });
            }
            dialogue.push_str(&word.text);
            previous_line = Some(word.line);
        }
        if !dialogue.is_empty() && inner.history.last() != Some(&dialogue) {
            inner.history.push(dialogue);
            if inner.history.len() > 12 {
                inner.history.remove(0);
            }
        }
        Ok(Frame {
            image,
            ocr,
            paused: settings.auto_pause,
            warning,
            history: inner.history.clone(),
            game_title: game.title,
            blocks,
        })
    }
    /// Resumes first and disables render hooks on ordinary application shutdown.
    pub fn cleanup(&self) {
        let _guard = self.operation.lock().unwrap();
        let _ = self.resume_inner();
        self.mini.stop();
        self.llm.stop();
        let inner = self.inner.lock().unwrap();
        if let Some(game) = inner.game.as_ref().filter(|_| inner.capture_mode == "hook") {
            let _ = native::call(&self.executable, &["detach".into(), game.pid.to_string()]);
        }
    }
    /// Places the interactive overlay exactly over the physical game client area.
    pub fn show_overlay(&self, app: &AppHandle) -> Result<(), String> {
        let (game, frame) = {
            let inner = self.inner.lock().unwrap();
            (
                inner.game.clone().ok_or("Нет выбранной игры")?,
                inner.last_frame.clone().ok_or("Сначала получите кадр")?,
            )
        };
        let window = app
            .get_webview_window("overlay")
            .ok_or("Окно оверлея не создано")?;
        window
            .set_position(tauri::PhysicalPosition::new(game.x, game.y))
            .map_err(|e| e.to_string())?;
        window
            .set_size(tauri::PhysicalSize::new(game.width, game.height))
            .map_err(|e| e.to_string())?;
        window.set_focusable(true).map_err(|e| e.to_string())?;
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        app.emit_to("overlay", "captured", frame)
            .map_err(|e| e.to_string())
    }
    /// Shows the already-created transparent webview before OCR or image encoding.
    fn show_loading(&self, app: &AppHandle, game: &Game, activate: bool) -> Result<(), String> {
        if self.cancel_capture.load(Ordering::SeqCst) {
            return Err("Захват отменён".into());
        }
        let window = app
            .get_webview_window("overlay")
            .ok_or("Нет окна оверлея")?;
        window
            .set_position(tauri::PhysicalPosition::new(game.x, game.y))
            .map_err(|e| e.to_string())?;
        window
            .set_size(tauri::PhysicalSize::new(game.width, game.height))
            .map_err(|e| e.to_string())?;
        app.emit_to(
            "overlay",
            "overlay-loading",
            serde_json::json!({"title":game.title}),
        )
        .map_err(|e| e.to_string())?;
        window.set_focusable(activate).map_err(|e| e.to_string())?;
        window.show().map_err(|e| e.to_string())?;
        if activate {
            window.set_focus().map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    /// Downloads the saved preset if requested, then preloads it without blocking the UI.
    pub async fn prepare_model(&self, app: &AppHandle) -> Result<(), String> {
        let bundled = app
            .path()
            .resource_dir()
            .map_err(|e| e.to_string())?
            .join("models/opus/grammar-ready.json");
        if !self.mini.status().installed && bundled.exists() {
            self.mini.install(app).await?;
        }
        for _ in 0..3 {
            let settings = self.inner.lock().unwrap().data.settings.translator.clone();
            if settings.provider != "mini" || !settings.preload {
                if self.mini.status().installed {
                    let mini = self.mini.clone();
                    let threads = llm::threads(&settings);
                    let (endpoint, token) =
                        tauri::async_runtime::spawn_blocking(move || mini.start_grammar(threads))
                            .await
                            .map_err(|e| e.to_string())??;
                    self.wait_model(&endpoint, &token).await?;
                }
                return Ok(());
            }
            if settings.mini_model == "opus" {
                if !self.mini.status().installed {
                    if !settings.auto_install {
                        return Ok(());
                    }
                    self.mini.install(app).await?;
                }
            } else if !self.llm.installed(&settings) {
                if !settings.auto_install {
                    return Ok(());
                }
                self.llm.install(app, &settings).await?;
            }
            if self.inner.lock().unwrap().data.settings.translator != settings {
                continue;
            }
            if settings.mini_model != "opus" && self.mini.status().installed {
                let mini = self.mini.clone();
                let threads = llm::threads(&settings);
                let (endpoint, token) =
                    tauri::async_runtime::spawn_blocking(move || mini.start(threads))
                        .await
                        .map_err(|e| e.to_string())??;
                self.wait_model(&endpoint, &token).await?;
            }
            let rt = self.clone();
            let selected_settings = settings.clone();
            let (endpoint, token) =
                tauri::async_runtime::spawn_blocking(move || rt.start_model(&settings))
                    .await
                    .map_err(|e| e.to_string())??;
            let ready = self.wait_model(&endpoint, &token).await;
            if self.inner.lock().unwrap().data.settings.translator != selected_settings {
                continue;
            }
            ready?;
            let _ = app.emit("model-ready", ());
            return Ok(());
        }
        Ok(())
    }
    /// Starts the selected model; the quick translator may coexist with the tutor.
    pub fn start_model(&self, settings: &TranslatorSettings) -> Result<(String, String), String> {
        if settings.mini_model == "opus" {
            self.llm.stop();
            self.mini.start(llm::threads(settings))
        } else {
            self.llm.start(settings)
        }
    }
    /// Waits for loaded weights before reporting readiness.
    pub async fn wait_model(&self, endpoint: &str, token: &str) -> Result<(), String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(|e| e.to_string())?;
        let health = format!(
            "{}/health",
            endpoint.trim_end_matches("/v1").trim_end_matches('/')
        );
        for _ in 0..600 {
            let mini_matches = self
                .mini
                .server
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|s| s.endpoint == endpoint);
            let llm_matches = self
                .llm
                .server
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|s| s.endpoint == endpoint);
            if !mini_matches && !llm_matches {
                return Err("Настройки модели изменились. Повторите перевод.".into());
            }
            if !self.mini.status().running && !self.llm.running() {
                return Err(
                    "Процесс модели завершился. Проверьте устройство и журнал модели.".into(),
                );
            }
            if client
                .get(&health)
                .bearer_auth(token)
                .send()
                .await
                .is_ok_and(|r| r.status().is_success())
            {
                let mut matched = false;
                {
                    if let Some(current) = self.mini.server.lock().unwrap().as_mut() {
                        if current.endpoint == endpoint && current.token == token {
                            current.ready = true;
                            matched = true;
                        }
                    }
                }
                {
                    if let Some(current) = self.llm.server.lock().unwrap().as_mut() {
                        if current.endpoint == endpoint && current.token == token {
                            current.ready = true;
                            matched = true;
                        }
                    }
                }
                if !matched {
                    return Err("Настройки модели изменились. Повторите перевод.".into());
                }
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err("Модель не успела загрузиться. Выберите меньшую модель или другое устройство.".into())
    }
}
