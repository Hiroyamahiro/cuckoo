use super::*;
use sound_ui::Action;
fn failed(code: impl Into<String>, uncertain: bool) -> Value {
    json!({"state":"failed","code":code.into(),"uncertain":uncertain})
}
fn library_result<T>(
    result: Result<
        std::sync::mpsc::Receiver<Result<T, sound_library::Error>>,
        sound_library::Error,
    >,
) -> Result<T, Value> {
    let rx = result.map_err(|e| failed(e.code, e.uncertain))?;
    rx.recv()
        .map_err(|_| failed("LibraryWorkerGone", true))?
        .map_err(|e| failed(e.code, e.uncertain))
}
#[tauri::command]
pub async fn list_audio_sources(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Value, String> {
    current_window(&app, &window)?;
    let state = app.state::<Arc<State>>().inner().clone();
    let library = state._sound_library.clone();
    let result = tauri::async_runtime::spawn_blocking(move || library.ui_catalog())
        .await
        .map_err(|e| e.to_string())?;
    Ok(match result {
        Ok(mut v) => {
            v["status"] = status(&state);
            v
        }
        Err(e) => {
            json!({"status":status(&state),"sources":[],"error":e.code,"uncertain":e.uncertain})
        }
    })
}
#[tauri::command]
pub fn audio_library_receipt(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    session_id: String,
    request_id: String,
) -> Result<Value, String> {
    current_window(&app, &window)?;
    let state = app.state::<Arc<State>>();
    let outcome = state.sound_ops.receipt(&session_id, &request_id)?;
    Ok(json!({"status":status(&state),"requestId":request_id,"operation":outcome}))
}
#[cfg(feature = "sound-ui-test-harness")]
static NEXT_PICK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
fn pick() -> Result<Option<sound_library::SelectedMp3>, sound_library::Error> {
    #[cfg(feature = "sound-ui-test-harness")]
    {
        let n = NEXT_PICK.swap(0, Ordering::SeqCst);
        if n > 0 {
            return sound_library::verification_pick(n);
        }
    }
    sound_library::SelectedMp3::pick()
}
#[cfg(feature = "sound-ui-test-harness")]
#[tauri::command]
pub fn sound_ui_experiment(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    picker: u64,
) -> Result<(), String> {
    current_window(&app, &window)?;
    if picker == 101 || picker == 102 {
        settings_disk::arm_ui_fault(picker);
        return Ok(());
    }
    if !(1..=6).contains(&picker) {
        return Err("InvalidPickerFixture".into());
    }
    NEXT_PICK.store(picker, Ordering::SeqCst);
    Ok(())
}
#[tauri::command]
pub fn audio_library_operation(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    session_id: String,
    base_revision: u64,
    request_id: String,
    action: Action,
) -> Result<Value, String> {
    current_window(&app, &window)?;
    let state = app.state::<Arc<State>>().inner().clone();
    if !state
        .sound_ops
        .begin(&session_id, base_revision, &request_id, &action)?
    {
        return Ok(
            json!({"status":status(&state),"requestId":request_id,"operation":state.sound_ops.receipt(&session_id,&request_id)?}),
        );
    }
    if let Err(code) = state.settings.check_revision(&session_id, base_revision) {
        state
            .sound_ops
            .finish(&request_id, json!({"state":"rejected","code":code}));
        changed(&app, &state);
        return Ok(
            json!({"status":status(&state),"requestId":request_id,"operation":state.sound_ops.receipt(&session_id,&request_id)?}),
        );
    }
    changed(&app, &state);
    let worker = state.clone();
    let app_worker = app.clone();
    let key = request_id.clone();
    let session = session_id.clone();
    let spawn = thread::Builder::new()
        .name("cuckoo-sound-ui".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                || -> Result<Value, Value> {
                    match action {
                        Action::Select { target, sound_id } => {
                            worker
                                .settings
                                .select_sound(&session, base_revision, &key, &target, &sound_id)
                                .map_err(|e| failed(e, false))?;
                            loop {
                                let r = worker
                                    .settings
                                    .wait_preset(&key, std::time::Duration::from_secs(5));
                                if r["state"] != "pending" {
                                    return Ok(r);
                                }
                            }
                        }
                        Action::Register {} => {
                            let Some(selected) = pick().map_err(|e| failed(e.code, e.uncertain))?
                            else {
                                return Ok(json!({"state":"cancelled"}));
                            };
                            let name = selected.registration_name()
                                .map_err(|e| failed(e.code, e.uncertain))?;
                            let sound = library_result(worker._sound_library.execute_at(
                                Some((&session, base_revision)),
                                move |store| {
                                    let generation = store.snapshot()?.generation;
                                    store.register(selected, &name, generation)
                                },
                            ))?;
                            Ok(json!({"state":"saved","id":sound.id}))
                        }
                        Action::Rename { id, name } => {
                            let renamed = id.clone();
                            library_result(worker._sound_library.execute_at(
                                Some((&session, base_revision)),
                                move |store| {
                                    let generation = store.snapshot()?.generation;
                                    store.rename(&renamed, &name, generation)
                                },
                            ))?;
                            Ok(json!({"state":"saved","id":id}))
                        }
                        Action::Unregister { id } => {
                            let removed = id.clone();
                            library_result(worker._sound_library.execute_at(
                                Some((&session, base_revision)),
                                move |store| {
                                    let generation = store.snapshot()?.generation;
                                    store.unregister(&removed, generation)
                                },
                            ))?;
                            Ok(json!({"state":"saved","id":id}))
                        }
                    }
                },
            ))
            .unwrap_or_else(|_| Err(failed("SoundOperationPanicked", true)));
            worker.sound_ops.finish(&key, result.unwrap_or_else(|e| e));
            changed(&app_worker, &worker);
        });
    if let Err(e) = spawn {
        state
            .sound_ops
            .finish(&request_id, failed(format!("WorkerUnavailable:{e}"), false));
        changed(&app, &state);
    }
    Ok(
        json!({"status":status(&state),"requestId":request_id,"operation":state.sound_ops.receipt(&session_id,&request_id)?}),
    )
}
#[tauri::command]
pub fn preview_audio_source(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    target: String,
    sound_id: Option<String>,
    repeat: Option<u8>,
    volume: Option<u8>,
) -> Result<u64, String> {
    current_window(&app, &window)?;
    let state = app.state::<Arc<State>>();
    let s = state.settings.snapshot();
    if state.sound_ops.snapshot()["busy"] == true {
        return Err("AudioManagementBusy".into());
    }
    if scheduler::next(Local::now(), &s)
        .is_some_and(|n| n - Local::now().timestamp_millis() <= 3000)
    {
        return Err("ChimeImminent".into());
    }
    let (id, playback_repeat, gain) = preview_plan(&s, &target, sound_id, repeat, volume)?;
    state.audio.toggle_preview(&id, playback_repeat, gain)
}

fn preview_plan(s: &Value, target: &str, sound_id: Option<String>, repeat: Option<u8>, volume: Option<u8>) -> Result<(String, u64, f32), String> {
    if repeat.is_some_and(|r| ![1, 2].contains(&r)) || volume.is_some_and(|v| v > 100) {
        return Err("InvalidPlayback".into());
    }
    let (id, playback_repeat) = match target {
        "hour" => (
            scheduler::route_sound(&s["audioRouting"]["onHour"],Local::now().hour()*60)
                .ok_or("InvalidRouting")?
                .to_string(),
            s["hourlyRepeat"].as_u64().unwrap_or(1),
        ),
        "halfHour" => (
            scheduler::route_sound(&s["audioRouting"]["onHalfHour"],Local::now().hour()*60+30)
                .ok_or("InvalidRouting")?
                .to_string(),
            s["halfHourlyRepeat"].as_u64().unwrap_or(1),
        ),
        "one" => (sound_id.ok_or("MissingSoundId")?, 1),
        "custom" => (sound_id.ok_or("MissingSoundId")?, repeat.unwrap_or(1) as u64),
        _ => return Err("InvalidTarget".into()),
    };
    let individual = if target == "custom" { json!(volume.unwrap_or(100)) }
        else { s["audioRouting"][match target { "hour" => "onHour", "halfHour" => "onHalfHour", _ => "" }]["volume"].clone() };
    let gain = scheduler::effective_volume(&s["volume"], &individual);
    let playback_repeat=voicevox::playback_repeat(&id,playback_repeat);
    Ok((id, playback_repeat, gain))
}
#[cfg(test)]
mod preview_plan_tests {
    use super::*;
    #[test]
    fn preview_uses_draft_repeat_and_individual_gain_without_double_master_gain() {
        let s = json!({"volume":50,"hourlyRepeat":2,"halfHourlyRepeat":1,"audioRouting":{"onHour":{"soundId":"builtin:cuckoo-clock","volume":80},"onHalfHour":{"soundId":"builtin:church-bell","volume":60}}});
        let (id, repeat, volume) = preview_plan(&s,"custom",Some("builtin:school-bell".into()),Some(2),Some(80)).unwrap();
        assert_eq!(id,"builtin:school-bell"); assert_eq!(repeat,2); assert_eq!(volume,0.4);
        assert_eq!(preview_plan(&s,"custom",Some(format!("voicevox:{}","a".repeat(64))),Some(2),Some(80)).unwrap().1,1);
        assert_eq!(preview_plan(&s,"hour",None,None,None).unwrap().2,0.4);
        assert_eq!(preview_plan(&s,"halfHour",None,None,None).unwrap().2,0.3);
        assert_eq!(preview_plan(&s,"custom",Some(id.clone()),Some(1),Some(0)).unwrap().2,0.0);
        assert!(preview_plan(&s,"custom",Some(id.clone()),Some(3),Some(80)).is_err());
        assert!(preview_plan(&s,"custom",Some(id),Some(1),Some(101)).is_err());
        assert!(preview_plan(&s,"custom",None,None,None).is_err());
    }
}
