use super::*;
use crate::sound_library;
#[test]
fn management_cas_and_known_failure_preserve_settings_and_failure_certainty() {
    let (library, sound, blob) =
        sound_library::Service::test_registered(crate::builtin_audio::ENTRIES[0].bytes);
    let root = blob
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let s = Arc::new(MemorySettings::default());
    let disk_root = root.clone();
    let h = s.start_with(move || Store::test(disk_root), || {}).unwrap();
    s.flush(Duration::from_secs(5)).unwrap();
    library.attach_settings(&s);
    let session = s.status()["settingsSync"]["sessionId"]
        .as_str()
        .unwrap()
        .to_string();
    s.apply(&session, 0, "volume", &json!({"volume":37}))
        .unwrap();
    assert!(s.check_revision(&session, 1).is_err());
    s.flush(Duration::from_secs(5)).unwrap();
    let original = std::fs::read(root.join("settings.json")).unwrap();
    let error = library
        .execute_at(Some((&session, 0)), |store| store.snapshot())
        .err()
        .unwrap();
    assert_eq!(error.code, "RevisionConflict");
    assert!(!error.uncertain);
    let error = library
        .execute_at(Some(("old-session", 1)), |store| store.snapshot())
        .err()
        .unwrap();
    assert_eq!(error.code, "SessionConflict");
    assert_eq!(std::fs::read(root.join("settings.json")).unwrap(), original);
    assert_eq!(s.snapshot()["volume"], 37);
    s.select_sound(&session, 1, "choose", "hour", &sound.id)
        .unwrap();
    assert_eq!(
        s.wait_preset("choose", Duration::from_secs(5))["state"],
        "saved"
    );
    let id = sound.id.clone();
    let error = library
        .execute_at(Some((&session, 2)), move |store| {
            let generation = store.snapshot()?.generation;
            store.unregister(&id, generation)
        })
        .unwrap()
        .recv()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code, "SoundSelected");
    assert!(!error.uncertain);
    assert_eq!(s.status()["settingsSync"]["mode"], "persistent");
    s.stop_worker();
    h.join().unwrap();
    drop(library);
    drop(s);
    assert!(root.starts_with(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("test-runtime/settings")
    ));
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn ui_actions_cannot_contain_paths_or_unknown_fields() {
    for value in [
        json!({"type":"register","name":"one","path":"C:/not-allowed.mp3"}),
        json!({"type":"select","target":"hour","soundId":"builtin:school-bell","repeat":4}),
        json!({"type":"unregister","id":"x","deleteFile":true}),
    ] {
        assert!(serde_json::from_value::<crate::sound_ui::Action>(value).is_err());
    }
}
