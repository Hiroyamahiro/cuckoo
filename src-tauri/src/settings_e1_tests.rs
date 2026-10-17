use super::*;
use crate::{builtin_audio, settings_model, sound_library};
#[test]
fn explicit_settings_backup_restore_proves_identity_and_preserves_new_catalog_entries() {
    let (library, sound, _, root) = fixture();
    let (s, h) = start(&root);
    library.attach_settings(&s);
    assert_eq!(
        select(&s, "select-backup", "hour", &sound.id)["state"],
        "saved"
    );
    for (key, volume) in [("v37", 37), ("v38", 38)] {
        let m = s.status()["settingsSync"].clone();
        s.apply(
            m["sessionId"].as_str().unwrap(),
            m["revision"].as_u64().unwrap(),
            key,
            &json!({"volume":volume}),
        )
        .unwrap();
        s.flush(Duration::from_secs(5)).unwrap();
    }
    let token = std::fs::read_dir(root.join("backups"))
        .unwrap()
        .filter_map(|e| {
            let e = e.unwrap();
            let d = settings_model::decode(&std::fs::read(e.path()).ok()?).ok()?;
            if d.audio.volume == 37 && d.audio.on_hour.sound_id == sound.id {
                Some(e.file_name().to_string_lossy().to_string())
            } else {
                None
            }
        })
        .next()
        .unwrap();
    let chosen = root.join("second-selected.mp3");
    std::fs::write(&chosen, builtin_audio::ENTRIES[1].bytes).unwrap();
    let chosen = sound_library::Service::test_selection(chosen);
    let second = library
        .execute(move |store| {
            let g = store.snapshot()?.generation;
            store.register(chosen, "second", g)
        })
        .unwrap()
        .recv()
        .unwrap()
        .unwrap();
    let m = s.status()["settingsSync"].clone();
    s.restore(
        m["sessionId"].as_str().unwrap(),
        m["revision"].as_u64().unwrap(),
        &token,
    )
    .unwrap();
    s.flush(Duration::from_secs(5)).unwrap();
    assert_eq!(s.snapshot()["volume"], 37);
    assert_eq!(s.snapshot()["audioRouting"]["onHour"]["soundId"], sound.id);
    let catalog = library
        .execute(|store| store.snapshot())
        .unwrap()
        .recv()
        .unwrap()
        .unwrap();
    assert_eq!(catalog.sounds.len(), 2);
    assert!(catalog.sounds.iter().any(|v| v.id == second.id));
    assert_eq!(
        select(&s, "deselect-backup", "hour", builtin_audio::DEFAULT_ID)["state"],
        "saved"
    );
    let removed = sound.id.clone();
    library
        .execute(move |store| {
            let g = store.snapshot()?.generation;
            store.unregister(&removed, g)
        })
        .unwrap()
        .recv()
        .unwrap()
        .unwrap();
    stop(s, h);
    drop(library);
    let mut disk = Store::test(root.clone()).unwrap();
    disk.load().unwrap();
    let before = std::fs::read(root.join("settings.json")).unwrap();
    assert_eq!(disk.restore(&token).unwrap_err().code, "UnknownId");
    assert_eq!(std::fs::read(root.join("settings.json")).unwrap(), before);
    drop(disk);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn first_registration_after_fresh_or_migrated_settings_commits_both_files() {
    for migration in [false, true] {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("test-runtime/settings")
            .join(Uuid::new_v4().to_string());
        std::fs::create_dir_all(&root).unwrap();
        if migration {
            std::fs::write(
                root.join("settings.json"),
                include_bytes!("../resources/default-settings.json"),
            )
            .unwrap();
        }
        let (s, h) = start(&root);
        let library = sound_library::Service::test_open(&root);
        library.attach_settings(&s);
        let source = root.join("explicit-selected.mp3");
        std::fs::write(&source, builtin_audio::ENTRIES[0].bytes).unwrap();
        let chosen = sound_library::Service::test_selection(source);
        let sound = library
            .execute(move |store| store.register(chosen, "first", 0))
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert_eq!(
            select(&s, "first-route", "hour", &sound.id)["state"],
            "saved"
        );
        stop(s, h);
        drop(library);
        let (s, h) = start(&root);
        assert_eq!(s.snapshot()["audioRouting"]["onHour"]["soundId"], sound.id);
        stop(s, h);
        std::fs::remove_file(root.join("sounds/catalog.json")).unwrap();
        let mut disk = Store::test(root.clone()).unwrap();
        assert!(matches!(disk.load().unwrap(), Loaded::Blocked(_)));
        drop(disk);
        std::fs::remove_dir_all(root).unwrap();
    }
}
#[test]
fn cold_audio_library_rejects_a_catalog_different_from_the_live_settings_snapshot() {
    let (library, sound, _, root) = fixture();
    let (s, h) = start(&root);
    drop(library);
    let path = root.join("sounds/catalog.json");
    let mut c: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    c["generation"] = json!(2);
    std::fs::write(&path, serde_json::to_vec(&c).unwrap()).unwrap();
    let cold = sound_library::Service::test_open(&root);
    cold.attach_settings(&s);
    assert_eq!(
        cold.pin_for_playback(&sound.id).err().unwrap().code,
        "CatalogGenerationMismatch"
    );
    stop(s, h);
    drop(cold);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn foreign_catalog_change_during_selection_is_not_published_as_success() {
    let (library, sound, _, root) = fixture();
    let mut disk = Store::test(root.clone()).unwrap();
    disk.load().unwrap();
    disk.commit(Document::default()).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let (resume, wait) = std::sync::mpsc::channel();
    disk.gate = Some((crate::settings_disk::Stage::FinalVerify, tx, wait));
    let s = Arc::new(MemorySettings::default());
    let h = s.start_with(move || Ok(disk), || {}).unwrap();
    s.flush(Duration::from_secs(5)).unwrap();
    let m = s.status()["settingsSync"].clone();
    s.select_sound(
        m["sessionId"].as_str().unwrap(),
        0,
        "race",
        "hour",
        &sound.id,
    )
    .unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let path = root.join("sounds/catalog.json");
    let mut c: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    c["generation"] = json!(2);
    std::fs::write(&path, serde_json::to_vec(&c).unwrap()).unwrap();
    resume.send(()).unwrap();
    let outcome = s.wait_preset("race", Duration::from_secs(5));
    assert_eq!(outcome["state"], "failed");
    assert_eq!(outcome["uncertain"], true);
    assert_eq!(
        s.snapshot()["audioRouting"]["onHour"]["soundId"],
        builtin_audio::DEFAULT_ID
    );
    assert_eq!(s.status()["settingsSync"]["mode"], "blocked");
    stop(s, h);
    drop(library);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn selection_holds_blob_and_does_not_publish_before_final_verification() {
    let (library, sound, blob, root) = fixture();
    let mut disk = Store::test(root.clone()).unwrap();
    disk.load().unwrap();
    disk.commit(Document::default()).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let (resume, wait) = std::sync::mpsc::channel();
    disk.gate = Some((crate::settings_disk::Stage::FinalVerify, tx, wait));
    let s = Arc::new(MemorySettings::default());
    let h = s.start_with(move || Ok(disk), || {}).unwrap();
    s.flush(Duration::from_secs(5)).unwrap();
    library.attach_settings(&s);
    let before = s.snapshot();
    let m = s.status()["settingsSync"].clone();
    s.select_sound(
        m["sessionId"].as_str().unwrap(),
        0,
        "pending",
        "hour",
        &sound.id,
    )
    .unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        s.wait_preset("pending", Duration::from_millis(5))["state"],
        "pending"
    );
    assert_eq!(s.snapshot(), before);
    assert!(std::fs::OpenOptions::new().write(true).open(&blob).is_err());
    assert!(library.execute(|store| store.snapshot()).is_err());
    assert!(s
        .select_sound(
            m["sessionId"].as_str().unwrap(),
            0,
            "second",
            "hour",
            builtin_audio::DEFAULT_ID
        )
        .is_err());
    resume.send(()).unwrap();
    assert_eq!(
        s.wait_preset("pending", Duration::from_secs(5))["state"],
        "saved"
    );
    assert_eq!(s.snapshot()["audioRouting"]["onHour"]["soundId"], sound.id);
    stop(s, h);
    drop(library);
    assert!(std::fs::OpenOptions::new().write(true).open(&blob).is_ok());
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn migration_failures_retain_original_bytes_and_do_not_claim_success() {
    use crate::settings_disk::Stage;
    for stage in [
        Stage::Backup,
        Stage::TempWrite,
        Stage::TempFlush,
        Stage::Replace,
        Stage::AfterReplace,
        Stage::FinalVerify,
    ] {
        let (library, _, blob, root) = fixture();
        let raw = include_bytes!("../resources/default-settings.json");
        std::fs::write(root.join("settings.json"), raw).unwrap();
        let mut disk = Store::test(root.clone()).unwrap();
        disk.fault = Some(stage);
        assert!(disk.load().is_err());
        assert!(std::fs::read_dir(root.join("backups")).unwrap().any(|e| {
            let e = e.unwrap();
            e.file_name()
                .to_string_lossy()
                .starts_with("settings.migration-v7-")
                && std::fs::read(e.path()).unwrap() == raw
        }));
        if !matches!(stage, Stage::AfterReplace | Stage::FinalVerify) {
            assert_eq!(std::fs::read(root.join("settings.json")).unwrap(), raw)
        }
        assert!(blob.exists());
        drop(disk);
        drop(library);
        std::fs::remove_dir_all(root).unwrap();
    }
}
#[test]
fn pair_crash_child() {
    let Ok(root) = std::env::var("CUCKOO_E1_CRASH_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    assert!(root.starts_with(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("test-runtime/settings")
    ));
    let mode = std::env::var("CUCKOO_E1_CRASH_STAGE").unwrap();
    let mut disk = Store::test(root.clone()).unwrap();
    match mode.as_str() {
        "settings-before" => disk.inject_process_exit(crate::settings_disk::Stage::TempWrite),
        "settings-after" => disk.inject_process_exit(crate::settings_disk::Stage::AfterReplace),
        _ => {}
    }
    let s = Arc::new(MemorySettings::default());
    let _h = s.start_with(move || Ok(disk), || {}).unwrap();
    s.flush(Duration::from_secs(5)).unwrap();
    let library = sound_library::Service::test_open(&root);
    library.attach_settings(&s);
    let _ = library
        .execute(move |store| {
            if mode == "catalog-before" {
                store.crash = Some(sound_library::Stage::BeforeReplace);
            }
            let c = store.snapshot()?;
            store.unregister(&c.sounds[0].id, c.generation)
        })
        .unwrap()
        .recv();
    panic!("fault did not exit process");
}
#[test]
fn process_exit_pairs_reconcile_without_promoting_partial_data() {
    for mode in ["catalog-before", "settings-before", "settings-after"] {
        let (library, _, blob, root) = fixture();
        drop(library);
        let mut disk = Store::test(root.clone()).unwrap();
        disk.load().unwrap();
        disk.commit(Document::default()).unwrap();
        drop(disk);
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "settings::e1_tests::pair_crash_child",
                "--nocapture",
            ])
            .env("CUCKOO_E1_CRASH_ROOT", &root)
            .env("CUCKOO_E1_CRASH_STAGE", mode)
            .status()
            .unwrap();
        assert_eq!(
            result.code(),
            Some(if mode == "catalog-before" { 73 } else { 77 })
        );
        let before = std::fs::read(root.join("settings.json")).unwrap();
        let catalog = std::fs::read(root.join("sounds/catalog.json")).unwrap();
        let mut disk = Store::test(root.clone()).unwrap();
        let loaded = disk.load().unwrap();
        if mode == "settings-before" {
            assert!(matches!(loaded, Loaded::Blocked(_)))
        } else {
            assert!(matches!(loaded, Loaded::Valid(_)))
        }
        assert_eq!(std::fs::read(root.join("settings.json")).unwrap(), before);
        assert_eq!(
            std::fs::read(root.join("sounds/catalog.json")).unwrap(),
            catalog
        );
        assert!(blob.exists());
        drop(disk);
        std::fs::remove_dir_all(root).unwrap();
    }
}
#[test]
fn future_and_malformed_schemas_do_not_overwrite_existing_data() {
    for change in [0, 1, 2, 3] {
        let (_, _, _, root) = fixture();
        let mut disk = Store::test(root.clone()).unwrap();
        disk.load().unwrap();
        let saved = disk.commit(Document::default()).unwrap();
        let backup = disk.candidates().unwrap();
        disk.commit(saved).unwrap();
        let mut value = serde_json::to_value(Document::default()).unwrap();
        match change {
            0 => value["schemaVersion"] = json!(9),
            1 => value["requiredCapabilities"] = json!(["future-capability"]),
            2 => value["audio"]["onHour"]["soundId"] = json!("builtin:unknown"),
            _ => value["audio"]["userSounds"] = json!([]),
        }
        let bytes = serde_json::to_vec(&value).unwrap();
        std::fs::write(root.join("settings.json"), &bytes).unwrap();
        drop(disk);
        let mut disk = Store::test(root.clone()).unwrap();
        assert!(matches!(disk.load().unwrap(), Loaded::Blocked(_)));
        assert_eq!(std::fs::read(root.join("settings.json")).unwrap(), bytes);
        if change < 2 {
            assert!(disk
                .restore(
                    &backup
                        .first()
                        .map(|c| c.token.clone())
                        .unwrap_or_else(|| disk.candidates().unwrap()[0].token.clone())
                )
                .is_err());
        }
        assert_eq!(std::fs::read(root.join("settings.json")).unwrap(), bytes);
        drop(disk);
        std::fs::remove_dir_all(root).unwrap();
    }
}
fn fixture() -> (
    sound_library::Service,
    sound_library::Sound,
    PathBuf,
    PathBuf,
) {
    let (library, sound, blob) =
        sound_library::Service::test_registered(builtin_audio::ENTRIES[0].bytes);
    let root = blob
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    (library, sound, blob, root)
}
fn start(root: &Path) -> (Arc<MemorySettings>, thread::JoinHandle<()>) {
    let root = root.to_path_buf();
    let s = Arc::new(MemorySettings::default());
    let h = s.start_with(move || Store::test(root), || {}).unwrap();
    s.flush(Duration::from_secs(5)).unwrap();
    (s, h)
}
fn stop(s: Arc<MemorySettings>, h: thread::JoinHandle<()>) {
    s.stop_worker();
    h.join().unwrap();
}
fn select(s: &MemorySettings, key: &str, target: &str, id: &str) -> Value {
    let m = s.status()["settingsSync"].clone();
    s.select_sound(
        m["sessionId"].as_str().unwrap(),
        m["revision"].as_u64().unwrap(),
        key,
        target,
        id,
    )
    .unwrap();
    s.wait_preset(key, Duration::from_secs(5))
}
#[test]
fn migration_preserves_v7_values_catalog_and_exact_backup() {
    let (library, sound, blob, root) = fixture();
    let mut v: Value = serde_json::from_str(include_str!(
        "../resources/default-settings.json"
    ))
    .unwrap();
    v["generation"] = json!(15);
    v["audio"]["volume"] = json!(37);
    v["audio"]["onHour"]["repeat"] = json!(1);
    v["schedule"]["slots"] = json!(vec![false; 48]);
    v["presets"] = json!([{"id":"preserved","name":"Original","slots":vec![true;48]}]);
    let raw = serde_json::to_vec(&v).unwrap();
    std::fs::write(root.join("settings.json"), &raw).unwrap();
    let original_blob = std::fs::read(&blob).unwrap();
    let mut disk = Store::test(root.clone()).unwrap();
    let Loaded::Valid(doc) = disk.load().unwrap() else {
        panic!("migration failed")
    };
    assert_eq!(doc.schema_version, 8);
    assert_eq!(doc.generation, 16);
    assert_eq!(doc.audio.volume, 37);
    assert_eq!(doc.audio.on_hour.repeat, 1);
    assert_eq!(doc.audio.on_hour.sound_id, builtin_audio::DEFAULT_ID);
    assert_eq!(doc.audio.on_half_hour.sound_id, builtin_audio::DEFAULT_ID);
    assert_eq!(doc.schedule.slots, vec![false; 48]);
    assert_eq!(doc.presets[0].id, "preserved");
    assert!(std::fs::read_dir(root.join("backups"))
        .unwrap()
        .any(|e| std::fs::read(e.unwrap().path()).unwrap() == raw));
    let cat = library
        .execute(|s| s.snapshot())
        .unwrap()
        .recv()
        .unwrap()
        .unwrap();
    assert_eq!(cat.sounds[0].id, sound.id);
    assert_eq!(cat.sounds[0].sha256, sound.sha256);
    assert_eq!(std::fs::read(blob).unwrap(), original_blob);
    drop(disk);
    drop(library);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn route_persistence_selected_and_pinned_unregister_and_unavailable_isolation() {
    let (library, sound, blob, root) = fixture();
    let (s, h) = start(&root);
    library.attach_settings(&s);
    assert_eq!(select(&s, "hour", "hour", &sound.id)["state"], "saved");
    assert_eq!(
        select(&s, "half", "halfHour", "builtin:church-bell")["state"],
        "saved"
    );
    let status = s.status();
    let m = &status["settingsSync"];
    assert!(s
        .select_sound(
            m["sessionId"].as_str().unwrap(),
            0,
            "stale",
            "hour",
            builtin_audio::DEFAULT_ID
        )
        .is_err());
    let id = sound.id.clone();
    let e = library
        .execute(move |store| {
            let g = store.snapshot()?.generation;
            store.unregister(&id, g)
        })
        .unwrap()
        .recv()
        .unwrap()
        .unwrap_err();
    assert_eq!(e.code, "SoundSelected");
    let unknown = format!("user:{}", Uuid::new_v4());
    assert_eq!(select(&s, "unknown", "hour", &unknown)["state"], "failed");
    assert_eq!(s.snapshot()["audioRouting"]["onHour"]["soundId"], sound.id);
    stop(s, h);
    let (s, h) = start(&root);
    library.attach_settings(&s);
    assert_eq!(s.snapshot()["audioRouting"]["onHour"]["soundId"], sound.id);
    assert_eq!(
        s.snapshot()["audioRouting"]["onHalfHour"]["soundId"],
        "builtin:church-bell"
    );
    let mut pin = library.pin_for_playback(&sound.id).unwrap();
    assert!(pin.decode(|| false).is_ok());
    assert_eq!(
        select(&s, "deselect", "hour", builtin_audio::DEFAULT_ID)["state"],
        "saved"
    );
    let id = sound.id.clone();
    assert_eq!(
        library
            .execute(move |store| {
                let g = store.snapshot()?.generation;
                store.unregister(&id, g)
            })
            .unwrap()
            .recv()
            .unwrap()
            .unwrap_err()
            .code,
        "SoundPinned"
    );
    drop(pin);
    assert_eq!(select(&s, "reselect", "hour", &sound.id)["state"], "saved");
    stop(s, h);
    std::fs::remove_file(&blob).unwrap();
    let (s, h) = start(&root);
    library.attach_settings(&s);
    assert_eq!(
        s.status()["audioRoutingAvailability"]["onHour"]["state"],
        "Unavailable"
    );
    assert_eq!(
        s.status()["audioRoutingAvailability"]["onHalfHour"]["state"],
        "Available"
    );
    assert_eq!(s.snapshot()["audioRouting"]["onHour"]["soundId"], sound.id);
    assert_eq!(select(&s, "missing", "hour", &sound.id)["state"], "failed");
    assert_eq!(
        select(&s, "resolve", "hour", builtin_audio::DEFAULT_ID)["state"],
        "saved"
    );
    stop(s, h);
    drop(library);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn catalog_operation_pairs_both_generations_and_rejects_external_catalog_change() {
    let (library, _, _, root) = fixture();
    let (s, h) = start(&root);
    library.attach_settings(&s);
    let before = std::fs::read(root.join("sounds/catalog.json")).unwrap();
    library
        .execute(|store| {
            let c = store.snapshot()?;
            store.unregister(&c.sounds[0].id, c.generation)
        })
        .unwrap()
        .recv()
        .unwrap()
        .unwrap();
    let settings: settings_model::Document =
        settings_model::decode(&std::fs::read(root.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings.catalog_ref.as_ref().unwrap().generation, 2);
    stop(s, h);
    // Emulate a noncooperating old catalog writer using ONLY this dummy profile.
    std::fs::write(root.join("sounds/catalog.json"), &before).unwrap();
    drop(library);
    let mut disk = Store::test(root.clone()).unwrap();
    assert!(matches!(disk.load().unwrap(), Loaded::Blocked(_)));
    drop(disk);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn catalog_and_settings_partial_commits_never_publish_success_or_auto_repair() {
    use crate::settings_disk::Stage;
    for stage in [
        Stage::Backup,
        Stage::TempCreate,
        Stage::TempWrite,
        Stage::TempFlush,
        Stage::TempVerify,
        Stage::Replace,
        Stage::AfterReplace,
        Stage::FinalVerify,
    ] {
        let (library, sound, blob, root) = fixture();
        let mut disk = Store::test(root.clone()).unwrap();
        disk.load().unwrap();
        disk.commit(Document::default()).unwrap();
        disk.fault = Some(stage);
        let s = Arc::new(MemorySettings::default());
        let h = s.start_with(move || Ok(disk), || {}).unwrap();
        s.flush(Duration::from_secs(5)).unwrap();
        library.attach_settings(&s);
        let original = s.snapshot();
        let id = sound.id.clone();
        let result = library
            .execute(move |store| {
                let g = store.snapshot()?.generation;
                store.unregister(&id, g)
            })
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(result.is_err());
        assert_eq!(s.snapshot(), original);
        assert_eq!(s.status()["settingsSync"]["canEdit"], false);
        assert!(blob.exists());
        assert!(library.execute(|store| store.snapshot()).is_err());
        stop(s, h);
        drop(library);
        let raw = std::fs::read(root.join("settings.json")).unwrap();
        let catalog_raw = std::fs::read(root.join("sounds/catalog.json")).unwrap();
        let mut disk = Store::test(root.clone()).unwrap();
        if matches!(stage, Stage::AfterReplace | Stage::FinalVerify) {
            assert!(matches!(disk.load().unwrap(), Loaded::Valid(_)))
        } else {
            assert!(matches!(disk.load().unwrap(), Loaded::Blocked(_)))
        }
        assert_eq!(std::fs::read(root.join("settings.json")).unwrap(), raw);
        assert_eq!(
            std::fs::read(root.join("sounds/catalog.json")).unwrap(),
            catalog_raw
        );
        assert!(blob.exists());
        drop(disk);
        std::fs::remove_dir_all(root).unwrap();
    }
}
#[test]
fn failed_route_change_is_not_applied_and_old_presets_do_not_change_audio() {
    use crate::settings_disk::Stage;
    for stage in [Stage::TempFlush, Stage::AfterReplace] {
        let (_, _, _, root) = fixture();
        let mut disk = Store::test(root.clone()).unwrap();
        disk.load().unwrap();
        disk.commit(Document::default()).unwrap();
        disk.fault = Some(stage);
        let s = Arc::new(MemorySettings::default());
        let h = s.start_with(move || Ok(disk), || {}).unwrap();
        s.flush(Duration::from_secs(5)).unwrap();
        assert_eq!(
            select(&s, "fail", "hour", "builtin:church-bell")["state"],
            "failed"
        );
        assert_eq!(
            s.snapshot()["audioRouting"]["onHour"]["soundId"],
            builtin_audio::DEFAULT_ID
        );
        stop(s, h);
        std::fs::remove_dir_all(root).unwrap();
    }
    let (_, _, _, root) = fixture();
    let (s, h) = start(&root);
    assert_eq!(
        select(&s, "route", "hour", "builtin:church-bell")["state"],
        "saved"
    );
    let m = s.status()["settingsSync"].clone();
    s.submit_preset(
        m["sessionId"].as_str().unwrap(),
        m["revision"].as_u64().unwrap(),
        "make",
        &json!({"type":"create","name":"E1"}),
    )
    .unwrap();
    let made = s.wait_preset("make", Duration::from_secs(5));
    assert_eq!(made["state"], "saved");
    let before = s.snapshot();
    let m = s.status()["settingsSync"].clone();
    s.submit_preset(
        m["sessionId"].as_str().unwrap(),
        m["revision"].as_u64().unwrap(),
        "apply",
        &json!({"type":"apply","id":made["id"]}),
    )
    .unwrap();
    assert_eq!(
        s.wait_preset("apply", Duration::from_secs(5))["state"],
        "saved"
    );
    assert_eq!(s.snapshot()["audioRouting"], before["audioRouting"]);
    stop(s, h);
    std::fs::remove_dir_all(root).unwrap();
}
