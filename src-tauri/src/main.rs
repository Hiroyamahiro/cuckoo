mod audio;
mod builtin_audio;
mod external_links;
mod lifecycle;
mod mp3_decode;
mod native_display;
mod scheduler;
mod settings;
mod settings_disk;
mod settings_model;
mod sound_commands;
mod sound_library;
mod sound_ui;
use chrono::{Local, Timelike};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};
struct State {
    sound_ops: sound_ui::Operations,
    // Lazy Rust-only management service; independent of WebView lifetimes.
    _sound_library: sound_library::Service,
    settings: Arc<settings::MemorySettings>,
    writer: Mutex<Option<thread::JoinHandle<()>>>,
    quit_pending: AtomicBool,
    audio: Arc<audio::Audio>,
    stop: AtomicBool,
    clock: Mutex<Option<thread::JoinHandle<()>>>,
    ui: Mutex<lifecycle::Lifecycle>,
    window_builds: AtomicU64,
    ticks: AtomicU64,
    tray_builds: AtomicU64,
    clock_starts: AtomicU64,
    fail_create: Vec<u64>,
    fail_after_build: Vec<u64>,
    display: Mutex<native_display::Display>,
    display_error: Mutex<String>,
}
fn status(state: &State) -> Value {
    let mut result = state.settings.status();
    let display_error = state.display_error.lock().unwrap().clone();
    result["nativeDisplayError"] = json!(display_error);
    if !display_error.is_empty() {
        result["settingsNotice"] = json!(format!(
            "{} ウィンドウ表示エラー：{display_error}",
            result["settingsNotice"].as_str().unwrap_or("")
        ));
    }
    let s = result["settings"].clone();
    result["soundManagement"] = state.sound_ops.snapshot();
    let ui = state.ui.lock().unwrap();
    let audio = state.audio.snapshot();
    let extra = json!({"enabled":s["enabled"],"next":scheduler::next(Local::now(),&s),"audioReady":audio.available,"message":audio.message,"audio":audio,"audioPcm":builtin_audio::pcm_stats(),"trayAvailable":true,"experiment":{"mode":ui.mode.name(),"phase":format!("{:?}",ui.phase),"label":ui.label,"attempts":ui.attempts,"windowBuilds":state.window_builds.load(Ordering::SeqCst),"schedulerTicks":state.ticks.load(Ordering::SeqCst),"trayBuilds":state.tray_builds.load(Ordering::SeqCst),"schedulerStarts":state.clock_starts.load(Ordering::SeqCst)}});
    result
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    result
}
fn changed(app: &tauri::AppHandle, state: &State) {
    let _ = app.emit("status", status(state));
    on_ui(app, sync_display);
}
fn sync_display(app: &tauri::AppHandle) {
    let state = app.state::<Arc<State>>();
    if state.stop.load(Ordering::SeqCst)
        || state.settings.status()["settingsSync"]["loaded"] != true
    {
        return;
    }
    let (attempts, label, phase) = {
        let ui = state.ui.lock().unwrap();
        (ui.attempts, ui.label.clone(), ui.phase)
    };
    if attempts == 0 {
        show(app);
        return;
    }
    if !matches!(phase, lifecycle::Phase::Present | lifecycle::Phase::Hidden) {
        return;
    }
    if let Some(w) = label.and_then(|l| app.get_webview_window(&l)) {
        let error = native_display::sync(app, &w).err().unwrap_or_default();
        let changed_error = {
            let mut old = state.display_error.lock().unwrap();
            let changed = *old != error;
            *old = error;
            changed
        };
        if changed_error {
            let _ = app.emit("status", status(&state));
        }
    }
}
fn current_window(app: &tauri::AppHandle, w: &tauri::WebviewWindow) -> Result<(), String> {
    let state = app.state::<Arc<State>>();
    let ui = state.ui.lock().unwrap();
    if state.quit_pending.load(Ordering::SeqCst)
        || ui.label.as_deref() != Some(w.label())
        || !matches!(
            ui.phase,
            lifecycle::Phase::Present | lifecycle::Phase::Hidden
        )
    {
        Err("ウィンドウは利用できません。".into())
    } else {
        Ok(())
    }
}
#[tauri::command]
async fn open_credit(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    link: String,
) -> Result<(), String> {
    current_window(&app, &window)?;
    external_links::target(&link)?;
    tauri::async_runtime::spawn_blocking(move || external_links::open(&link))
        .await
        .map_err(|e| e.to_string())?
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
enum WindowAction {
    State,
    Drag,
    Minimize,
    Maximize,
    Close,
}
#[tauri::command]
async fn window_action(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    action: WindowAction,
) -> Result<Value, String> {
    current_window(&app, &window)?;
    let (tx, rx) = std::sync::mpsc::channel();
    let target = app.clone();
    app.run_on_main_thread(move || {
        let result = current_window(&target, &window).and_then(|_| {
            match action {
                WindowAction::State => (),
                WindowAction::Drag => window.start_dragging().map_err(|e| e.to_string())?,
                WindowAction::Minimize => window.minimize().map_err(|e| e.to_string())?,
                WindowAction::Maximize => {
                    if window.is_maximized().map_err(|e| e.to_string())? {
                        window.unmaximize()
                    } else {
                        window.maximize()
                    }
                    .map_err(|e| e.to_string())?;
                }
                WindowAction::Close => window.close().map_err(|e| e.to_string())?,
            };
            Ok(json!({"maximized":window.is_maximized().unwrap_or(false)}))
        });
        let _ = tx.send(result);
    })
    .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        rx.recv()
            .map_err(|_| "ウィンドウ操作の結果を確認できません。".to_string())?
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn fit_minimum_display(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    height: f64,
) -> Result<(), String> {
    native_display::valid_height(height)?;
    current_window(&app, &window)?;
    let (tx, rx) = std::sync::mpsc::channel();
    let target = app.clone();
    app.run_on_main_thread(move || {
        let result = current_window(&target, &window)
            .and_then(|_| native_display::fit(&target, &window, height));
        let state = target.state::<Arc<State>>();
        *state.display_error.lock().unwrap() = result.as_ref().err().cloned().unwrap_or_default();
        let _ = tx.send(result);
    })
    .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| "表示変更の完了を確認できません。".to_string())?
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn get_status(state: tauri::State<Arc<State>>) -> Value {
    status(&state)
}
#[tauri::command]
fn update_settings(
    app: tauri::AppHandle,
    state: tauri::State<Arc<State>>,
    session_id: String,
    base_revision: u64,
    request_id: String,
    patch: Value,
) -> Result<Value, String> {
    if state.sound_ops.snapshot()["busy"] == true {
        return Err("AudioManagementBusy".into());
    }
    state
        .settings
        .apply(&session_id, base_revision, &request_id, &patch)?;
    changed(&app, &state);
    Ok(status(&state))
}
#[tauri::command]
async fn edit_preset(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<State>>,
    session_id: String,
    base_revision: u64,
    request_id: String,
    action: Value,
) -> Result<Value, String> {
    if state.sound_ops.snapshot()["busy"] == true {
        return Ok(
            json!({"status":status(&state),"operation":{"state":"rejected","code":"AudioManagementBusy"},"requestId":request_id}),
        );
    }
    if let Err(code) =
        state
            .settings
            .submit_preset(&session_id, base_revision, &request_id, &action)
    {
        return Ok(
            json!({"status":status(&state),"operation":{"state":"rejected","code":code},"requestId":request_id}),
        );
    }
    changed(&app, &state);
    let settings = state.settings.clone();
    let key = request_id.clone();
    let operation = tauri::async_runtime::spawn_blocking(move || {
        settings.wait_preset(&key, std::time::Duration::from_secs(5))
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(json!({"status":status(&state),"operation":operation,"requestId":request_id}))
}
#[tauri::command]
fn retry_settings(state: tauri::State<Arc<State>>) -> Result<(), String> {
    state.settings.retry()
}
#[tauri::command]
fn restore_settings(
    state: tauri::State<Arc<State>>,
    session_id: String,
    base_revision: u64,
    token: String,
) -> Result<(), String> {
    state.settings.restore(&session_id, base_revision, &token)
}
fn submit_routed(
    state: &State,
    automatic: bool,
    repeat: u64,
    volume: f32,
    key: Option<&str>,
    deadline: Option<std::time::Instant>,
    id: &str,
) -> Result<u64, String> {
    if sound_library::id_valid(id) {
        state
            .audio
            .submit_custom(automatic, repeat, volume, key, deadline, id)
    } else {
        state
            .audio
            .submit_builtin(automatic, repeat, volume, key, deadline, id)
    }
}
fn play(state: &State, repeat: u64) -> Result<u64, String> {
    let s = state.settings.snapshot();
    let volume = s["volume"].as_f64().unwrap_or(50.) as f32 / 100.;
    let id = s["audioRouting"]["onHour"]["soundId"]
        .as_str()
        .unwrap_or(builtin_audio::DEFAULT_ID);
    if state.sound_ops.snapshot()["busy"] == true {
        return Err("AudioManagementBusy".into());
    }
    state.audio.toggle_preview(id, repeat, volume)
}
#[tauri::command]
async fn select_audio_source(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Arc<State>>,
    session_id: String,
    base_revision: u64,
    request_id: String,
    target: String,
    sound_id: String,
) -> Result<Value, String> {
    current_window(&app, &window)?;
    state
        .settings
        .select_sound(&session_id, base_revision, &request_id, &target, &sound_id)?;
    let settings = state.settings.clone();
    tauri::async_runtime::spawn_blocking(move || {
        settings.wait_preset(&request_id, std::time::Duration::from_secs(5))
    })
    .await
    .map_err(|e| e.to_string())
}
#[tauri::command]
fn test_sound(
    state: tauri::State<'_, Arc<State>>,
    sound_id: Option<String>,
) -> Result<u64, String> {
    let s = state.settings.snapshot();
    if scheduler::next(Local::now(), &s)
        .map(|n| n - Local::now().timestamp_millis() <= 3000)
        .unwrap_or(false)
    {
        return Err("自動時報の3秒前のためテスト再生できません".into());
    }
    if state.sound_ops.snapshot()["busy"] == true {
        return Err("AudioManagementBusy".into());
    }
    let repeat = s["hourlyRepeat"].as_u64().unwrap_or(2);
    let id = sound_id.unwrap_or_else(|| {
        s["audioRouting"]["onHour"]["soundId"]
            .as_str()
            .unwrap_or(builtin_audio::DEFAULT_ID)
            .to_string()
    });
    state.audio.toggle_preview(
        &id,
        repeat,
        s["volume"].as_f64().unwrap_or(50.) as f32 / 100.,
    )
}
#[tauri::command]
fn audio_receipt(state: tauri::State<'_, Arc<State>>, id: u64) -> Option<audio::Receipt> {
    state.audio.receipt(id)
}
#[cfg(feature = "audio-test-harness")]
#[tauri::command]
fn audio_experiment(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    mode: String,
    minute: Option<u32>,
    sound_id: Option<String>,
) -> Result<Value, String> {
    current_window(&app, &window)?;
    let state = app.state::<Arc<State>>();
    match mode.as_str() {
        "custom-cancel" => {
            state.audio.cancel();
            Ok(json!(true))
        }
        "device-failure" => {
            state.audio.inject(1);
            Ok(json!(true))
        }
        "decode-failure" => {
            state.audio.inject(2);
            Ok(json!(true))
        }
        "hold-test" => {
            state.audio.inject(3);
            Ok(json!(true))
        }
        "corrupt-next" => {
            state.audio.inject(4);
            Ok(json!(true))
        }
        "custom-hold-prepare" => {
            state.audio.inject(6);
            Ok(json!(true))
        }
        "custom-stuck" => {
            state.audio.inject(8);
            Ok(json!(true))
        }
        "custom-release" => {
            state.audio.inject(9);
            Ok(json!(true))
        }
        "custom-decode-failure" => {
            state.audio.inject(7);
            Ok(json!(true))
        }
        "custom-device-failure" => {
            state.audio.inject(10);
            Ok(json!(true))
        }
        "custom-playing-failure" => {
            state.audio.inject(11);
            Ok(json!(true))
        }
        "mix-failure" => {
            state.audio.inject(5);
            Ok(json!(true))
        }
        "catalog" => Ok(serde_json::from_str(include_str!(
            "../../assets/sounds/builtin_mp3/manifest.json"
        ))
        .unwrap()),
        "automatic" => {
            let minute = minute
                .filter(|v| [0, 30].contains(v))
                .ok_or("InvalidMinute")?;
            let s = state.settings.snapshot();
            let repeat = scheduler::repeat_for(minute, &s);
            let key = format!("injected-{}", uuid::Uuid::new_v4());
            eprintln!("AUDIO_INJECTED_AUTOMATIC minute={minute}");
            let volume = s["volume"].as_f64().unwrap_or(50.) as f32 / 100.;
            let deadline = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
            match sound_id {
                Some(id) if sound_library::id_valid(&id) => {
                    state
                        .audio
                        .submit_custom(true, repeat, volume, Some(&key), deadline, &id)
                }
                Some(id) => {
                    state
                        .audio
                        .submit_builtin(true, repeat, volume, Some(&key), deadline, &id)
                }
                None => submit_routed(
                    &state,
                    true,
                    repeat,
                    volume,
                    Some(&key),
                    deadline,
                    s["audioRouting"][if minute == 30 { "onHalfHour" } else { "onHour" }]
                        ["soundId"]
                        .as_str()
                        .unwrap_or(builtin_audio::DEFAULT_ID),
                ),
            }
            .map(|id| json!(id))
        }
        _ => Err("InvalidAudioExperiment".into()),
    }
}
#[cfg(feature = "custom-playback-test-harness")]
#[tauri::command]
async fn custom_audio_experiment(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    mode: String,
    id: Option<String>,
) -> Result<Value, String> {
    current_window(&app, &window)?;
    let library = app.state::<Arc<State>>()._sound_library.clone();
    if mode == "pins" {
        return library.pin_count().map(|n| json!(n)).map_err(|e| e.code);
    }
    let receiver = library.verification(mode, id).map_err(|e| e.code)?;
    tauri::async_runtime::spawn_blocking(move || {
        receiver
            .recv()
            .map_err(|_| "LibraryWorkerDisconnected".to_string())?
            .map_err(|e| e.code)
    })
    .await
    .map_err(|e| e.to_string())?
}
// Always enqueue from a pool worker, including when called from Destroyed.
// This lets the runtime remove the old window before reusing its label.
fn on_ui(app: &tauri::AppHandle, action: fn(&tauri::AppHandle)) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let target = app.clone();
        if let Err(e) = app.run_on_main_thread(move || action(&target)) {
            eprintln!("UI dispatch: {e}");
        }
    });
}
fn show(app: &tauri::AppHandle) {
    on_ui(app, show_on_ui)
}
fn show_on_ui(app: &tauri::AppHandle) {
    let state = app.state::<Arc<State>>();
    if state.stop.load(Ordering::SeqCst) {
        return;
    }
    if state.settings.status()["settingsSync"]["loaded"] != true {
        return;
    }
    // Failed cleanup retains its generation until Destroyed. Never build beside it.
    let cleanup = {
        let ui = state.ui.lock().unwrap();
        if ui.phase == lifecycle::Phase::Failed {
            ui.label.clone()
        } else {
            None
        }
    };
    if let Some(label) = cleanup {
        state.ui.lock().unwrap().retry_cleanup();
        if let Some(w) = app.get_webview_window(&label) {
            if let Err(e) = w.destroy() {
                state.ui.lock().unwrap().cleanup_failed();
                eprintln!("UI cleanup: {e}");
            }
        } else {
            let reopen = state.ui.lock().unwrap().destroyed(&label);
            if reopen {
                show(app)
            }
        }
        return;
    }
    let Some((label, create)) = state.ui.lock().unwrap().begin_show() else {
        return;
    };
    if create {
        eprintln!("UI_ATTEMPT {label}");
    }
    let result = if create {
        let attempt = state.ui.lock().unwrap().attempts;
        if state.fail_create.contains(&attempt) {
            Err("Injected before WebView builder".to_string())
        } else {
            let mut config = app.config().app.windows[0].clone();
            config.label = label.clone();
            config.visible = false;
            tauri::WebviewWindowBuilder::from_config(app, &config)
                .map(|b| {
                    b.on_navigation(external_links::navigation_allowed)
                        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
                })
                .map(|b| {
                    #[cfg(feature = "settings-test-harness")]
                    {
                        b.data_directory(settings_disk::verification_root().join("EBWebView"))
                    }
                    #[cfg(not(feature = "settings-test-harness"))]
                    {
                        b
                    }
                })
                .and_then(|b| b.build())
                .map_err(|e| e.to_string())
                .and_then(|w| {
                    state.window_builds.fetch_add(1, Ordering::SeqCst);
                    eprintln!("UI_CREATED {label}");
                    if state.fail_after_build.contains(&attempt) {
                        Err("Injected after WebView builder".to_string())
                    } else {
                        Ok(w)
                    }
                })
        }
    } else {
        app.get_webview_window(&label)
            .ok_or_else(|| "Window reference missing".to_string())
    };
    match result {
        Ok(w) => {
            let native = if create {
                native_display::attach(app, &w)
            } else {
                native_display::sync(app, &w)
            };
            if let Err(e) = native {
                *state.display_error.lock().unwrap() = e.clone();
                fail_ui(app, &label, &e);
                return;
            }
            if let Err(e) = w
                .show()
                .and_then(|_| {
                    if w.is_minimized()? {
                        w.unminimize()
                    } else {
                        Ok(())
                    }
                })
                .and_then(|_| w.set_focus())
            {
                fail_ui(app, &label, &e.to_string());
            } else {
                state.ui.lock().unwrap().shown(true);
                eprintln!("UI_PRESENT {label}");
            }
        }
        Err(e) => fail_ui(app, &label, &e),
    }
}
fn fail_ui(app: &tauri::AppHandle, label: &str, error: &str) {
    let state = app.state::<Arc<State>>();
    let partial = app.get_webview_window(label);
    state.ui.lock().unwrap().create_failed(partial.is_some());
    eprintln!(
        "UI_FAILED {label} partial={} error={error}",
        partial.is_some()
    );
    if let Some(w) = partial {
        if let Err(e) = w.destroy() {
            state.ui.lock().unwrap().cleanup_failed();
            eprintln!("UI cleanup: {e}");
        }
    }
}
fn store(app: &tauri::AppHandle) {
    on_ui(app, store_on_ui)
}
fn store_on_ui(app: &tauri::AppHandle) {
    let state = app.state::<Arc<State>>();
    if state.stop.load(Ordering::SeqCst) {
        return;
    }
    let target = {
        let mut ui = state.ui.lock().unwrap();
        if !matches!(
            ui.phase,
            lifecycle::Phase::Present | lifecycle::Phase::Hidden
        ) {
            return;
        }
        let label = ui.label.clone();
        ui.stored();
        (label, ui.mode)
    };
    if let Some(label) = target.0 {
        if let Some(w) = app.get_webview_window(&label) {
            if let Ok(o) = native_display::observed(&w) {
                state.display.lock().unwrap().observe(w.label(), o);
            }
            let result = match target.1 {
                lifecycle::Mode::Hide => w.hide(),
                lifecycle::Mode::Destroy => w.destroy(),
            };
            if let Err(e) = result {
                state.ui.lock().unwrap().shown(true);
                eprintln!("UI store: {e}");
            }
        }
    }
}
fn set_mode(app: &tauri::AppHandle, mode: lifecycle::Mode) {
    app.state::<Arc<State>>().ui.lock().unwrap().mode = mode;
    eprintln!("EXPERIMENT_MODE {}", mode.name());
}
fn experiment_action(app: &tauri::AppHandle, args: &[String]) {
    match lifecycle::options(args) {
        Ok(options) => {
            if let Some(mode) = options.mode {
                set_mode(app, mode)
            }
            match options.action {
                lifecycle::Action::Open => show(app),
                lifecycle::Action::Store => store(app),
                lifecycle::Action::Quit => shutdown(app),
                lifecycle::Action::Test => {
                    let state = app.state::<Arc<State>>();
                    if let Err(e) = play(
                        &state,
                        state.settings.snapshot()["hourlyRepeat"]
                            .as_u64()
                            .unwrap_or(2),
                    ) {
                        eprintln!("experiment audio: {e}");
                    }
                }
            }
        }
        Err(e) => eprintln!("{e}"),
    }
}
fn shutdown(app: &tauri::AppHandle) {
    let state = app.state::<Arc<State>>();
    if state.quit_pending.swap(true, Ordering::SeqCst) {
        return;
    }
    state.settings.closing(true);
    changed(app, &state);
    let st = state.inner().clone();
    let app = app.clone();
    thread::spawn(move || {
        loop {
            match st.settings.flush(std::time::Duration::from_secs(5)) {
                Ok(()) => break,
                Err(e) => {
                    let text=format!("設定の保存に失敗、または保存処理が継続中です：{e}\n再試行しますか？ キャンセルすると常駐を継続します。");
                    if native_dialog(
                        &text,
                        windows_sys::Win32::UI::WindowsAndMessaging::MB_RETRYCANCEL,
                    ) != windows_sys::Win32::UI::WindowsAndMessaging::IDRETRY
                    {
                        st.settings.closing(false);
                        st.quit_pending.store(false, Ordering::SeqCst);
                        changed(&app, &st);
                        return;
                    }
                    let _ = st.settings.retry();
                }
            }
        }
        if !st.audio.shutdown(std::time::Duration::from_secs(2)) {
            // Do not join a stuck OS call, pretend cleanup succeeded, or stop the clock.
            eprintln!("AUDIO_SHUTDOWN_PENDING: worker termination unconfirmed");
            native_dialog("音声処理の停止を確認できないため、終了を中止しました。時計・トレイ・設定管理は継続します。停止を確認するまで新しい音声再生は開始できません。",windows_sys::Win32::UI::WindowsAndMessaging::MB_OK|windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONERROR);
            st.settings.closing(false);
            st.quit_pending.store(false, Ordering::SeqCst);
            changed(&app, &st);
            return;
        }
        st.stop.store(true, Ordering::SeqCst);
        st.settings.stop_worker();
        if let Some(h) = st.writer.lock().unwrap().take() {
            let _ = h.join();
        }
        if let Some(h) = st.clock.lock().unwrap().take() {
            let _ = h.join();
        }
        app.exit(0);
    });
}
fn native_dialog(text: &str, flags: u32) -> i32 {
    let text = text.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let title = "Cuckoo 設定"
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            flags | windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONWARNING,
        )
    }
}
fn recover_settings(app: &tauri::AppHandle) {
    let app = app.clone();
    thread::spawn(move || {
        let st = app.state::<Arc<State>>();
        let status = st.settings.status();
        let sync = &status["settingsSync"];
        if sync["canRestore"] != true {
            native_dialog("現在はバックアップ復旧できません。保存中の場合は完了を待ち、成否不明の場合は再起動して確認してください。",0);
            return;
        }
        let Some(candidate) = sync["backupCandidates"].as_array().and_then(|a| a.first()) else {
            native_dialog("検証済みバックアップはありません。", 0);
            return;
        };
        let text=format!("検証済みバックアップ（保存世代 {}）から復旧しますか？ 現在の設定は退避し、復旧内容を新しい世代として保存します。未保存の変更は置き換わります。",candidate["generation"]);
        if native_dialog(&text, windows_sys::Win32::UI::WindowsAndMessaging::MB_YESNO)
            == windows_sys::Win32::UI::WindowsAndMessaging::IDYES
        {
            if let Err(e) = st.settings.restore(
                sync["sessionId"].as_str().unwrap(),
                sync["revision"].as_u64().unwrap(),
                candidate["token"].as_str().unwrap(),
            ) {
                native_dialog(&format!("復旧要求に失敗しました：{e}"), 0);
            }
        }
    });
}
#[tauri::command]
fn quit(app: tauri::AppHandle) {
    shutdown(&app)
}
fn main() {
    let context = tauri::generate_context!();
    settings::check_identifier(&context.config().identifier)
        .expect("Application identifier mismatch");
    let options = lifecycle::options(&std::env::args().collect::<Vec<_>>())
        .expect("Invalid T2-B2 experiment options");
    let sound_library = sound_library::Service::default();
    let state = Arc::new(State {
        sound_ops: Default::default(),
        _sound_library: sound_library.clone(),
        settings: Arc::new(settings::MemorySettings::default()),
        writer: Mutex::new(None),
        quit_pending: AtomicBool::new(false),
        audio: Arc::new(audio::Audio::with_library(sound_library)),
        stop: AtomicBool::new(false),
        clock: Mutex::new(None),
        ui: Mutex::new(lifecycle::Lifecycle::new(options.mode.unwrap_or_default())),
        window_builds: AtomicU64::new(0),
        ticks: AtomicU64::new(0),
        tray_builds: AtomicU64::new(0),
        clock_starts: AtomicU64::new(0),
        fail_create: options.fail_create,
        fail_after_build: options.fail_after_build,
        display: Mutex::new(Default::default()),
        display_error: Mutex::new(String::new()),
    });
    state._sound_library.attach_settings(&state.settings);
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            experiment_action(app, &args)
        }))
        .manage(state)
        .invoke_handler(|invoke: tauri::ipc::Invoke<tauri::Wry>| {
            #[cfg(feature = "custom-playback-test-harness")]
            if invoke.message.command() == "custom_audio_experiment" {
                let library_handler: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool =
                    tauri::generate_handler![custom_audio_experiment];
                return library_handler(invoke);
            }
            #[cfg(feature = "audio-test-harness")]
            if invoke.message.command() == "audio_experiment" {
                let handler: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool =
                    tauri::generate_handler![audio_experiment];
                return handler(invoke);
            }
            #[cfg(feature = "sound-ui-test-harness")]
            if invoke.message.command() == "sound_ui_experiment" {
                let handler: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool =
                    tauri::generate_handler![sound_commands::sound_ui_experiment];
                return handler(invoke);
            }
            let handler: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool = tauri::generate_handler![
                sound_commands::list_audio_sources,
                sound_commands::audio_library_operation,
                sound_commands::audio_library_receipt,
                sound_commands::preview_audio_source,
                get_status,
                update_settings,
                edit_preset,
                select_audio_source,
                open_credit,
                fit_minimum_display,
                window_action,
                retry_settings,
                restore_settings,
                test_sound,
                audio_receipt,
                quit
            ];
            handler(invoke)
        })
        .setup(|app| {
            let open = MenuItem::with_id(app, "open", "設定を開く", true, None::<&str>)?;
            let test = MenuItem::with_id(app, "test", "テスト再生", true, None::<&str>)?;
            let toggle = MenuItem::with_id(app, "toggle", "時報 ON/OFF", true, None::<&str>)?;
            let exit = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
            let retry = MenuItem::with_id(
                app,
                "settings-retry",
                "設定保存を再試行",
                true,
                None::<&str>,
            )?;
            let recover = MenuItem::with_id(
                app,
                "settings-recover",
                "バックアップから設定を復旧",
                true,
                None::<&str>,
            )?;
            #[cfg(feature = "settings-test-harness")]
            let hide_mode =
                MenuItem::with_id(app, "mode-hide", "検証: 非表示方式", true, None::<&str>)?;
            #[cfg(feature = "settings-test-harness")]
            let destroy_mode = MenuItem::with_id(
                app,
                "mode-destroy",
                "検証: 破棄・再生成方式",
                true,
                None::<&str>,
            )?;
            let menu = Menu::with_items(
                app,
                &[
                    &open,
                    &test,
                    &toggle,
                    &retry,
                    &recover,
                    &exit,
                ],
            )?;
            #[cfg(feature = "settings-test-harness")]
            { menu.append(&hide_mode)?; menu.append(&destroy_mode)?; }
            TrayIconBuilder::new()
                .tooltip("Cuckoo")
                .icon(tauri::image::Image::from_bytes(include_bytes!(
                    "../../assets/icons/app/cuckoo-app.ico"
                ))?)
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    let state = app.state::<Arc<State>>();
                    match event.id().as_ref() {
                        "open" => show(app),
                        "quit" => shutdown(app),
                        "mode-hide" => set_mode(app, lifecycle::Mode::Hide),
                        "mode-destroy" => set_mode(app, lifecycle::Mode::Destroy),
                        "test" => {
                            let st = state.inner().clone();
                            tauri::async_runtime::spawn(async move {
                                let s = st.settings.snapshot();
                                if scheduler::next(Local::now(), &s)
                                    .map(|n| n - Local::now().timestamp_millis() > 3000)
                                    .unwrap_or(true)
                                {
                                    if let Err(e) =
                                        play(&st, s["hourlyRepeat"].as_u64().unwrap_or(2))
                                    {
                                        eprintln!("{e}")
                                    }
                                }
                            });
                        }
                        "settings-retry" => {
                            let _ = state.settings.retry();
                        }
                        "settings-recover" => recover_settings(app),
                        "toggle" => {
                            if let Err(e) = state.settings.toggle() {
                                eprintln!("settings: {e}")
                            }
                            changed(app, &state);
                        }
                        _ => (),
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } | TrayIconEvent::DoubleClick {
                            button: MouseButton::Left,
                            ..
                        }
                    ) {
                        show(tray.app_handle())
                    }
                })
                .build(app)?;
            let st = app.state::<Arc<State>>().inner().clone();
            let audio_app = app.handle().clone();
            let audio_state = st.clone();
            if let Err(e) = st.audio.start(move || {
                let _ = audio_app.emit("status", status(&audio_state));
            }) {
                eprintln!("audio coordinator: {e}");
            }
            st.tray_builds.fetch_add(1, Ordering::SeqCst);
            let worker = st.clone();
            st.clock_starts.fetch_add(1, Ordering::SeqCst);
            let handle = thread::Builder::new()
                .name("cuckoo-clock".into())
                .spawn(move || {
                    let mut scheduler = scheduler::Scheduler::default();
                    let mut pending: Option<(String, u64, f32, std::time::Instant, String)> = None;
                    while !worker.stop.load(Ordering::SeqCst) {
                        worker.ticks.fetch_add(1, Ordering::SeqCst);
                        let now = Local::now();
                        let s = worker.settings.snapshot();
                        let (fire, gap) = scheduler.poll(now, std::time::Instant::now(), &s);
                        if gap && worker.ticks.load(Ordering::SeqCst) > 1 {
                            worker.audio.cancel();
                            pending = None;
                        }
                        if fire {
                            eprintln!("CHIME {}", now.to_rfc3339());
                            let repeat = scheduler::repeat_for(now.minute(), &s);
                            let lateness =
                                now.second() as u64 * 1000 + now.timestamp_subsec_millis() as u64;
                            pending = Some((
                                now.format("%Y-%m-%d-%H-%M").to_string(),
                                repeat,
                                s["volume"].as_f64().unwrap_or(50.) as f32 / 100.,
                                std::time::Instant::now()
                                    + std::time::Duration::from_millis(
                                        2000u64.saturating_sub(lateness),
                                    ),
                                s["audioRouting"][if now.minute() == 30 {
                                    "onHalfHour"
                                } else {
                                    "onHour"
                                }]["soundId"]
                                    .as_str()
                                    .unwrap_or(builtin_audio::DEFAULT_ID)
                                    .to_string(),
                            ));
                        }
                        if let Some((key, repeat, volume, deadline, sound_id)) = pending.take() {
                            if std::time::Instant::now() > deadline {
                                eprintln!("CHIME_SKIPPED {key} DeadlineExceeded");
                            } else if let Err(e) = submit_routed(
                                &worker,
                                true,
                                repeat,
                                volume,
                                Some(&key),
                                Some(deadline),
                                &sound_id,
                            ) {
                                if e == "AudioMailboxBusy" {
                                    pending = Some((key, repeat, volume, deadline, sound_id));
                                } else {
                                    eprintln!("chime: {e}");
                                }
                            }
                        }
                        if worker.ticks.load(Ordering::SeqCst) % 20 == 0 {
                            eprintln!("BACKEND_TICK {}", worker.ticks.load(Ordering::SeqCst));
                        }
                        thread::sleep(scheduler::TICK);
                    }
                })?;
            *st.clock.lock().unwrap() = Some(handle);
            let app_handle = app.handle().clone();
            let notify_state = st.clone();
            match st
                .settings
                .start(move || changed(&app_handle, &notify_state))
            {
                Ok(h) => *st.writer.lock().unwrap() = Some(h),
                Err(e) => {
                    eprintln!("settings worker: {e}");
                    changed(app.handle(), &st);
                }
            }
            eprintln!("BACKEND_READY");
            show(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
                let app = window.app_handle();
                if let Some(w) = app.get_webview_window(window.label()) {
                    if let Ok(o) = native_display::observed(&w) {
                        app.state::<Arc<State>>()
                            .display
                            .lock()
                            .unwrap()
                            .observe(w.label(), o);
                    }
                }
            }
            tauri::WindowEvent::ScaleFactorChanged { .. } => {
                let _ = window.emit("native-display-refit", ());
            }
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                store(window.app_handle());
            }
            tauri::WindowEvent::Destroyed => {
                let app = window.app_handle();
                let reopen = app
                    .state::<Arc<State>>()
                    .ui
                    .lock()
                    .unwrap()
                    .destroyed(window.label());
                eprintln!("UI_DESTROYED {}", window.label());
                if reopen {
                    show(app)
                }
            }
            _ => (),
        })
        .build(context)
        .expect("Tauri startup failed")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
                if code.is_some() && !app.state::<Arc<State>>().stop.load(Ordering::SeqCst) {
                    api.prevent_exit();
                    shutdown(app);
                    return;
                }
                if lifecycle::keep_alive(
                    code,
                    app.state::<Arc<State>>().stop.load(Ordering::SeqCst),
                ) {
                    api.prevent_exit();
                }
            }
        });
}
