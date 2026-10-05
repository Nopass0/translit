//! Translit Tauri host. Native integration is restricted to the selected game.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod bindings;
mod context;
mod decision;
mod dictionary;
mod downloads;
mod grammar;
mod input;
mod learning;
mod llm;
mod mini;
mod models;
mod native;
mod ocr;
mod runtime;
mod subtitles;
use models::*;
use runtime::Runtime;
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};
use tauri::{Emitter, Manager, State};

/// Returns the application state and the complete persisted personal dictionary.
#[tauri::command]
fn get_status(state: State<Runtime>) -> Status {
    state.status()
}
/// Lists visible process windows and detected capture capabilities.
#[tauri::command]
async fn list_games(state: State<'_, Runtime>) -> Result<Vec<Game>, String> {
    let exe = state.executable.clone();
    tauri::async_runtime::spawn_blocking(move || {
        serde_json::from_str(&native::call(&exe, &["list".into()])?).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
/// Injects the bundled DLL into a revalidated process and attaches its session.
#[tauri::command]
async fn attach_game(pid: u32, state: State<'_, Runtime>) -> Result<(), String> {
    let rt = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = rt.operation.lock().unwrap();
        let games: Vec<Game> =
            serde_json::from_str(&native::call(&rt.executable, &["list".into()])?)
                .map_err(|e| e.to_string())?;
        let game = games
            .into_iter()
            .find(|g| g.pid == pid)
            .ok_or("Выбранная игра уже закрыта")?;
        rt.resume_inner()?;
        {
            let mut inner = rt.inner.lock().unwrap();
            if let Some(old) = inner.game.take() {
                if inner.capture_mode == "hook" {
                    native::call(&rt.executable, &["detach".into(), old.pid.to_string()])?;
                }
            }
        }
        let requested = rt
            .inner
            .lock()
            .unwrap()
            .data
            .settings
            .capture_backend
            .clone();
        let mode =
            if requested == "screen" || requested == "auto" && (game.api != "dx11" || !game.x64) {
                "screen"
            } else {
                "hook"
            };
        if mode == "hook" {
            if !game.x64 {
                return Err(
                    "DLL адаптер поддерживает x64. Выберите захват окна для 32-битной игры.".into(),
                );
            }
            native::call(
                &rt.executable,
                &[
                    "inject".into(),
                    pid.to_string(),
                    rt.dll.to_string_lossy().into_owned(),
                ],
            )?;
        }
        let mut inner = rt.inner.lock().unwrap();
        inner.game = Some(game);
        inner.capture_mode = mode.into();
        inner.history.clear();
        inner.last_frame = None;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}
/// Captures a frame, temporarily pauses the game, and returns OCR hitboxes.
#[tauri::command]
async fn capture_frame(app: tauri::AppHandle, state: State<'_, Runtime>) -> Result<Frame, String> {
    let rt = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || rt.capture(&app))
        .await
        .map_err(|e| e.to_string())?
}
/// Releases the pause watchdog and returns keyboard focus to the selected game.
#[tauri::command]
async fn resume_game(app: tauri::AppHandle, state: State<'_, Runtime>) -> Result<(), String> {
    let rt = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        rt.cancel_capture
            .store(true, std::sync::atomic::Ordering::SeqCst);
        rt.resume_inner()?;
        if let Some(window) = app.get_webview_window("overlay") {
            let _ = window.hide();
        }
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_always_on_top(false);
            let _ = window.hide();
        }
        if let Some(game) = rt.inner.lock().unwrap().game.as_ref() {
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow(game.hwnd as _);
            }
        }
        let _ = app.emit("status-changed", ());
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}
/// Disables our render hooks and clears the selected process after resuming it.
#[tauri::command]
async fn detach_game(state: State<'_, Runtime>) -> Result<(), String> {
    let rt = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = rt.operation.lock().unwrap();
        rt.resume_inner()?;
        let mut inner = rt.inner.lock().unwrap();
        if let Some(game) = inner.game.take() {
            if inner.capture_mode == "hook" {
                native::call(&rt.executable, &["detach".into(), game.pid.to_string()])?;
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}
/// Looks up a normalized token locally, prioritizing the user's personal entries.
#[tauri::command]
fn lookup_word(word: String, state: State<Runtime>) -> Definition {
    dictionary::lookup(&state.dictionary, &state.inner.lock().unwrap().data, &word)
}
/// Returns local phrase evidence immediately while a model request is pending.
#[tauri::command]
fn local_translation(
    selection: String,
    context: String,
    state: State<Runtime>,
) -> Result<ContextTranslation, String> {
    let mut result = context::local(&state, &selection, &context);
    learning::record(&state, &mut result, &context)?;
    Ok(result)
}
/// Annotates a sentence without counting another dictionary lookup.
#[tauri::command]
async fn analyze_sentence(
    context: String,
    state: State<'_, Runtime>,
) -> Result<GrammarAnalysis, String> {
    grammar::analyze(&state, &context).await
}
/// Lists real phrase request counters and grammatical construction frequencies.
#[tauri::command]
fn get_statistics(state: State<Runtime>) -> learning::Statistics {
    learning::statistics(&state)
}
/// Validates and persists one vocabulary entry, replacing the same normalized word.
#[tauri::command]
fn save_entry(mut entry: Entry, state: State<Runtime>) -> Result<(), String> {
    entry.word = dictionary::normalize(&entry.word);
    entry.translation = entry.translation.trim().into();
    if entry.word.is_empty() || entry.translation.is_empty() {
        return Err("Введите слово и перевод".into());
    }
    if entry.word.len() > 200 || entry.translation.len() > 8000 || entry.context.len() > 16000 {
        return Err("Запись слишком большая".into());
    }
    let mut inner = state.inner.lock().unwrap();
    let previous = serde_json::to_vec(&inner.data).map_err(|e| e.to_string())?;
    entry.query_count = inner
        .data
        .lookups
        .iter()
        .find(|s| s.phrase == entry.word)
        .map(|s| s.count)
        .unwrap_or(entry.query_count);
    if let Some(existing) = inner.data.entries.iter().find(|e| e.word == entry.word) {
        entry.review_count = existing.review_count;
        entry.learned = existing.learned;
        entry.due_at = existing.due_at;
        entry.interval_days = existing.interval_days;
        entry.streak = existing.streak;
        entry.lapses = existing.lapses;
        entry.last_reviewed = existing.last_reviewed;
        if entry.analysis.is_none() {
            entry.analysis = existing.analysis.clone();
        }
    }
    if let Some(analysis) = entry.analysis.as_mut() {
        if serde_json::to_vec(analysis)
            .map_err(|e| e.to_string())?
            .len()
            > 32000
        {
            return Err("Разбор слишком большой".into());
        }
        analysis.translation = entry.translation.clone();
    }
    entry.created_at = inner
        .data
        .entries
        .iter()
        .find(|e| e.word == entry.word)
        .map(|e| e.created_at)
        .unwrap_or(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        );
    inner.data.entries.retain(|e| e.word != entry.word);
    inner.data.entries.insert(0, entry);
    if let Err(error) = dictionary::persist(&state.store, &inner.data) {
        inner.data = serde_json::from_slice(&previous).unwrap();
        return Err(error);
    }
    Ok(())
}
/// Removes a personal entry after persisting the replacement store successfully.
#[tauri::command]
fn delete_entry(word: String, state: State<Runtime>) -> Result<(), String> {
    let mut inner = state.inner.lock().unwrap();
    let old = serde_json::to_vec(&inner.data).unwrap();
    inner.data.entries.retain(|e| e.word != word);
    if let Err(e) = dictionary::persist(&state.store, &inner.data) {
        inner.data = serde_json::from_slice(&old).unwrap();
        return Err(e);
    }
    Ok(())
}
/// Records a flashcard review without discarding the saved context or grammar analysis.
#[tauri::command]
fn review_entry(word: String, learned: bool, state: State<Runtime>) -> Result<(), String> {
    let mut inner = state.inner.lock().unwrap();
    let previous = serde_json::to_vec(&inner.data).map_err(|e| e.to_string())?;
    let entry = inner
        .data
        .entries
        .iter_mut()
        .find(|e| e.word == word)
        .ok_or("Запись не найдена")?;
    entry.review_count = entry.review_count.saturating_add(1);
    entry.learned = learned;
    crate::learning::schedule(entry, learned);
    if let Err(error) = dictionary::persist(&state.store, &inner.data) {
        inner.data = serde_json::from_slice(&previous).unwrap();
        return Err(error);
    }
    Ok(())
}
/// Validates and stores keyboard/controller preferences.
#[tauri::command]
fn save_settings(
    app: tauri::AppHandle,
    settings: Settings,
    state: State<Runtime>,
) -> Result<(), String> {
    if !["auto", "hook", "screen"].contains(&settings.capture_backend.as_str())
        || !["offline", "mini", "ollama", "openai", "compatible"]
            .contains(&settings.translator.provider.as_str())
        || settings.translator.history_lines > 12
        || ![
            "opus",
            "qwen35-small",
            "qwen35-2b",
            "qwen3-4b",
            "bonsai-1.7b",
        ]
        .contains(&settings.translator.mini_model.as_str())
        || !["cpu", "vulkan"].contains(&settings.translator.device.as_str())
        || settings.translator.threads > 64
        || settings.decision.threads > 8
        || !(256..=1024).contains(&settings.decision.tokens)
        || !(250..=2000).contains(&settings.subtitles.interval_ms)
        || !(18..=56).contains(&settings.subtitles.font_size)
        || !(0.35..=0.85).contains(&settings.subtitles.region_top)
        || !(0.1..=0.45).contains(&settings.subtitles.region_height)
    {
        return Err("Некорректные настройки переводчика".into());
    }
    if !["offline", "mini"].contains(&settings.translator.provider.as_str()) {
        let url = reqwest::Url::parse(&settings.translator.endpoint)
            .map_err(|_| "Некорректный адрес API")?;
        if !["http", "https"].contains(&url.scheme())
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err("Укажите HTTP/HTTPS адрес без ключей в URL".into());
        }
    }
    if bindings::keyboard(&settings.hotkey).is_none()
        || bindings::gamepad(&settings.gamepad).is_none()
    {
        return Err("Неизвестная кнопка".into());
    }
    let mut inner = state.inner.lock().unwrap();
    let old = inner.data.settings.clone();
    let changed_model = old.translator != settings.translator;
    let changed_decision = old.decision != settings.decision;
    inner.data.settings = settings;
    if let Err(error) = dictionary::persist(&state.store, &inner.data) {
        inner.data.settings = old;
        return Err(error);
    }
    drop(inner);
    let _ = app.emit("status-changed", ());
    if changed_model {
        state.translation_cache.lock().unwrap().clear();
        state.mini.stop();
        state.llm.stop();
        let rt = state.inner().clone();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = rt.prepare_model(&app).await {
                let _ = app.emit("app-error", error);
            }
        });
    }
    if changed_decision {
        state.decision.stop();
        let rt = state.inner().clone();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = rt.decision.prepare(&rt, &app, false).await {
                let _ = app.emit("app-error", error);
            }
        });
    }
    Ok(())
}
/// Returns the latest frame for a newly opened overlay webview.
#[tauri::command]
fn get_frame(state: State<Runtime>) -> Option<Frame> {
    state.inner.lock().unwrap().last_frame.clone()
}
/// Reopens the interactive overlay on the last captured game frame.
#[tauri::command]
fn show_overlay(app: tauri::AppHandle, state: State<Runtime>) -> Result<(), String> {
    state.show_overlay(&app)
}
/// Opens the main dictionary window while keeping an existing game pause.
#[tauri::command]
fn show_main(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.hide();
    }
}
/// Translates a phrase using current dialogue and previous captured lines.
#[tauri::command]
async fn translate_selection(
    selection: String,
    context: String,
    state: State<'_, Runtime>,
) -> Result<ContextTranslation, String> {
    context::translate(state.inner().clone(), selection, context).await
}
/// Returns the quick bundled neural translation without waiting for tutor generation.
#[tauri::command]
async fn fast_translation(
    selection: String,
    context: String,
    state: State<'_, Runtime>,
) -> Result<ContextTranslation, String> {
    context::fast(state.inner().clone(), selection, context).await
}
/// Reports the managed mini-model installation and process state.
#[tauri::command]
fn mini_status(state: State<Runtime>) -> mini::MiniStatus {
    let settings = state.inner.lock().unwrap().data.settings.translator.clone();
    if settings.mini_model == "opus" {
        let mut status = state.mini.status();
        status.threads = llm::threads(&settings);
        status
    } else {
        let model = llm::catalog()
            .into_iter()
            .find(|m| m.id == settings.mini_model);
        mini::MiniStatus {
            installed: state.llm.installed(&settings),
            running: state.llm.running(),
            ready: state
                .llm
                .server
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|s| s.ready),
            parser_only: false,
            model: model.as_ref().map(|m| m.name.clone()).unwrap_or_default(),
            size_mb: model.map(|m| (m.size / 1048576) as u32).unwrap_or(0),
            threads: llm::threads(&settings),
            device: settings.device,
            grammar: true,
            installing: state
                .llm
                .installing
                .load(std::sync::atomic::Ordering::SeqCst),
        }
    }
}
/// Lists pinned grammar model presets and public source links for the settings panel.
#[tauri::command]
fn model_catalog() -> Vec<llm::Model> {
    llm::catalog()
}
/// Downloads and verifies the pinned CPU engine and miniature model.
#[tauri::command]
async fn install_mini(app: tauri::AppHandle, state: State<'_, Runtime>) -> Result<(), String> {
    let settings = state.inner.lock().unwrap().data.settings.translator.clone();
    if settings.mini_model == "opus" {
        state.mini.install(&app).await
    } else {
        state.llm.install(&app, &settings).await
    }
}
/// Loads the managed miniature model without contacting a cloud API.
#[tauri::command]
async fn start_mini(state: State<'_, Runtime>) -> Result<(), String> {
    let rt = state.inner().clone();
    let settings = state.inner.lock().unwrap().data.settings.translator.clone();
    let (endpoint, token) = tauri::async_runtime::spawn_blocking(move || rt.start_model(&settings))
        .await
        .map_err(|e| e.to_string())??;
    state.wait_model(&endpoint, &token).await
}
/// Unloads the owned inference process to free memory for the game.
#[tauri::command]
fn stop_mini(state: State<Runtime>) {
    state.mini.stop();
    state.llm.stop();
}
/// Reports whether the managed Laya model is installed, loading, or ready.
#[tauri::command]
fn decision_status(state: State<Runtime>) -> decision::Status {
    state.decision.status()
}
/// Installs the pinned Laya multilingual model and its private ONNX runtime.
#[tauri::command]
async fn install_decision(app: tauri::AppHandle, state: State<'_, Runtime>) -> Result<(), String> {
    state.decision.install(&app).await
}
/// Starts the Laya model without changing the active translation model.
#[tauri::command]
async fn start_decision(app: tauri::AppHandle, state: State<'_, Runtime>) -> Result<(), String> {
    let runtime = state.inner().clone();
    state.decision.prepare(&runtime, &app, true).await
}
/// Stops Laya and releases its inference process.
#[tauri::command]
fn stop_decision(state: State<Runtime>) {
    state.decision.stop();
}
/// Checks whether the user's API key is present in Windows Credential Manager.
#[tauri::command]
fn key_status(state: State<Runtime>) -> Result<bool, String> {
    Ok(native::call(&state.executable, &["key-status".into()])? == "saved")
}
/// Stores a key through stdin; it never appears in process arguments or JSON settings.
#[tauri::command]
fn save_api_key(key: String, state: State<Runtime>) -> Result<(), String> {
    use std::{
        io::Write,
        os::windows::process::CommandExt,
        process::{Command, Stdio},
    };
    if key.trim().is_empty() {
        native::call(&state.executable, &["key-clear".into()])?;
        return Ok(());
    }
    let mut child = Command::new(&state.executable)
        .arg("key-set")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|e| e.to_string())?;
    child
        .stdin
        .take()
        .ok_or("Нет канала для ключа")?
        .write_all(key.trim().as_bytes())
        .map_err(|e| e.to_string())?;
    let result = child.wait_with_output().map_err(|e| e.to_string())?;
    if !result.status.success() {
        return Err("Не удалось сохранить ключ в Credential Manager".into());
    }
    Ok(())
}
/// Exports vocabulary as UTF-8 JSON to the user's Documents folder.
#[tauri::command]
fn export_dictionary(app: tauri::AppHandle, state: State<Runtime>) -> Result<String, String> {
    let path = app
        .path()
        .document_dir()
        .map_err(|e| e.to_string())?
        .join(format!(
            "translit-words-{}.json",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        ));
    fs::write(
        &path,
        serde_json::to_vec_pretty(&state.inner.lock().unwrap().data.entries).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}
/// Resolves a development resource first, then its packaged application location.
fn resource(
    app: &tauri::AppHandle,
    relative: &str,
    development: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let local = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(development);
    if cfg!(debug_assertions) && local.exists() {
        return Ok(local);
    }
    Ok(app.path().resource_dir()?.join(relative))
}
/// Starts the desktop host and registers shutdown recovery before accepting input.
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .setup(|app| {
            let handle = app.handle().clone();
            let data = handle.path().app_data_dir()?;
            fs::create_dir_all(&data)?;
            let rt = Runtime::new(
                resource(
                    &handle,
                    "bin/translit_native.exe",
                    "resources/bin/translit_native.exe",
                )?,
                resource(
                    &handle,
                    "bin/translit_hook_v2.dll",
                    "resources/bin/translit_hook_v2.dll",
                )?,
                data.join("vocabulary.json"),
                resource(&handle, "data/eng-rus.json", "../data/eng-rus.json")?,
            )
            .map_err(std::io::Error::other)?;
            app.manage(rt.clone());
            subtitles::start(handle.clone(), rt.clone());
            let model_rt = rt.clone();
            let model_app = handle.clone();
            tauri::async_runtime::spawn(async move {
                if !model_rt.mini.status().installed {
                    if let Err(error) = model_rt.mini.install(&model_app).await {
                        let _ = model_app.emit("app-error", error);
                    }
                }
                let translator_rt = model_rt.clone();
                let translator_app = model_app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = translator_rt.prepare_model(&translator_app).await {
                        let _ = translator_app.emit("app-error", error);
                    }
                });
                if let Err(error) = model_rt
                    .decision
                    .prepare(&model_rt, &model_app, false)
                    .await
                {
                    let _ = model_app.emit("app-error", error);
                }
            });
            let show = MenuItem::with_id(app, "show", "Открыть словарь", true, None::<&str>)?;
            let resume = MenuItem::with_id(app, "resume", "Продолжить игру", true, None::<&str>)?;
            let exit = MenuItem::with_id(app, "exit", "Выйти", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &resume, &exit])?;
            let mut tray = TrayIconBuilder::new()
                .menu(&menu)
                .tooltip("Translit · игровой словарь");
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.on_menu_event(|app, event| match event.id.as_ref() {
                "show" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                "resume" => {
                    let rt = app.state::<Runtime>().inner().clone();
                    let handle = app.clone();
                    tauri::async_runtime::spawn_blocking(move || {
                        let _guard = rt.operation.lock().unwrap();
                        if let Err(error) = rt.resume_inner() {
                            let _ = handle.emit("app-error", error);
                            return;
                        }
                        if let Some(window) = handle.get_webview_window("overlay") {
                            let _ = window.hide();
                        }
                        if let Some(game) = rt.inner.lock().unwrap().game.as_ref() {
                            unsafe {
                                windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow(
                                    game.hwnd as _,
                                );
                            }
                        }
                        let _ = handle.emit("status-changed", ());
                    });
                }
                "exit" => {
                    let rt = app.state::<Runtime>().inner().clone();
                    let handle = app.clone();
                    tauri::async_runtime::spawn_blocking(move || {
                        rt.cleanup();
                        handle.exit(0);
                    });
                }
                _ => {}
            })
            .build(app)?;
            input::start(handle, rt);
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "subtitles" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
                return;
            }
            if window.label() == "overlay" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                    let rt = window.state::<Runtime>().inner().clone();
                    let app = window.app_handle().clone();
                    tauri::async_runtime::spawn_blocking(move || {
                        let _guard = rt.operation.lock().unwrap();
                        let _ = rt.resume_inner();
                        let _ = app.emit("status-changed", ());
                    });
                }
                return;
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let rt = window.state::<Runtime>().inner().clone();
                let app = window.app_handle().clone();
                tauri::async_runtime::spawn_blocking(move || {
                    rt.cleanup();
                    app.exit(0);
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            list_games,
            attach_game,
            capture_frame,
            resume_game,
            detach_game,
            lookup_word,
            local_translation,
            analyze_sentence,
            get_statistics,
            save_entry,
            delete_entry,
            review_entry,
            save_settings,
            export_dictionary,
            get_frame,
            show_overlay,
            show_main,
            translate_selection,
            fast_translation,
            mini_status,
            model_catalog,
            install_mini,
            start_mini,
            stop_mini,
            decision_status,
            install_decision,
            start_decision,
            stop_decision,
            key_status,
            save_api_key
        ])
        .run(tauri::generate_context!())
        .expect("Не удалось запустить Translit");
}
