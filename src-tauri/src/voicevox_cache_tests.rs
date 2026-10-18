use super::*;
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("cuckoo-voicevox-gc-{}", uuid::Uuid::new_v4()));
        paths::directory_chain(&root.join("voicevox")).unwrap();
        paths::directory_chain(&root.join("backups")).unwrap();
        let t = Self(root);
        t.settings(&crate::settings_model::Document::default());
        t
    }
    fn settings(&self, doc: &crate::settings_model::Document) {
        fs::write(
            self.0.join("settings.json"),
            serde_json::to_vec(doc).unwrap(),
        )
        .unwrap();
    }
    fn file(&self, name: &str) -> PathBuf {
        self.0.join("voicevox").join(name)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
}
fn day(n: u64) -> SystemTime {
    now() + Duration::from_secs(n * 86400)
}
fn speech() -> Speech {
    Speech {
        character: "zundamon".into(),
        style_id: None,
        character_name: None,
        style_name: None,
        policy: None,
        mode: "text".into(),
        text: uuid::Uuid::new_v4().to_string(),
    }
}
fn seed(t: &Temp) -> (String, Speech) {
    let s = speech();
    let recipe = key(&s, 0).unwrap();
    let mut wav = Vec::new();
    wav.extend(b"RIFF");
    wav.extend(38u32.to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(48000u32.to_le_bytes());
    wav.extend(96000u32.to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(2u32.to_le_bytes());
    wav.extend(0i16.to_le_bytes());
    let sha = hash(&wav);
    let digest = hash(format!("{recipe}:{sha}").as_bytes());
    let id = format!("voicevox:{digest}");
    publish(&t.0.join("voicevox"), &format!("{digest}.wav"), &wav).unwrap();
    publish(
        &t.0.join("voicevox"),
        &format!("{digest}.json"),
        &serde_json::to_vec(&json!({"key":recipe,"soundId":id,"wavSha256":sha})).unwrap(),
    )
    .unwrap();
    (id, s)
}
fn doc(ids: &[(&String, &Speech)]) -> crate::settings_model::Document {
    crate::settings_model::Document::default().patched(&json!({"customChimes":ids.iter().enumerate().map(|(i,(id,s))|json!({"id":uuid::Uuid::new_v4().to_string(),"minute":i,"enabled":i%2==0,"soundId":id,"speech":s,"repeat":1,"volume":100})).collect::<Vec<_>>()})).unwrap()
}
fn exists(t: &Temp, id: &str) -> bool {
    path(&t.0, id, "wav").unwrap().exists()
}
fn sweep(t: &Temp, time: SystemTime) -> Report {
    let _gen = GENERATION.lock().unwrap();
    cleanup_at(&t.0, time).unwrap()
}

#[test]
fn grace_starts_when_first_unreferenced_not_from_file_age() {
    let t = Temp::new();
    let (id, _) = seed(&t);
    for ext in ["wav", "json"] {
        fs::OpenOptions::new()
            .write(true)
            .open(path(&t.0, &id, ext).unwrap())
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(now() - Duration::from_secs(100 * 86400)))
            .unwrap();
    }
    assert_eq!(sweep(&t, now()).files_deleted, 0);
    assert!(exists(&t, &id));
    assert_eq!(sweep(&t, day(7) - Duration::from_secs(1)).files_deleted, 0);
    assert_eq!(sweep(&t, day(7)).files_deleted, 2);
    assert!(!exists(&t, &id));
}
#[test]
fn shared_disabled_and_backup_references_reset_grace() {
    let t = Temp::new();
    let (a, sa) = seed(&t);
    let (b, sb) = seed(&t);
    sweep(&t, now());
    t.settings(&doc(&[(&a, &sa), (&a, &sa)]));
    fs::write(
        t.0.join("backups/settings.g1-test.json"),
        serde_json::to_vec(&doc(&[(&b, &sb)])).unwrap(),
    )
    .unwrap();
    sweep(&t, day(6));
    assert!(exists(&t, &a));
    assert!(exists(&t, &b));
    t.settings(&doc(&[(&a, &sa)]));
    sweep(&t, day(8));
    assert!(exists(&t, &a));
    assert!(exists(&t, &b));
    t.settings(&doc(&[]));
    fs::remove_file(t.0.join("backups/settings.g1-test.json")).unwrap();
    sweep(&t, day(9));
    sweep(&t, day(15));
    assert!(exists(&t, &a));
    assert!(exists(&t, &b));
    sweep(&t, day(16));
    assert!(!exists(&t, &a));
    assert!(!exists(&t, &b));
}
#[test]
fn editors_and_playback_remain_protected_until_release() {
    let t = Temp::new();
    let (a, _) = seed(&t);
    let (b, _) = seed(&t);
    sweep(&t, now());
    let session = uuid::Uuid::new_v4().to_string();
    editor(&t.0, &session, vec![a.clone()]).unwrap();
    let pin = playing(&t.0, &b).unwrap();
    sweep(&t, day(8));
    assert!(exists(&t, &a) && exists(&t, &b));
    editor(&t.0, &session, vec![]).unwrap();
    drop(pin);
    sweep(&t, day(9));
    sweep(&t, day(15));
    assert!(exists(&t, &a) && exists(&t, &b));
    sweep(&t, day(16));
    assert!(!exists(&t, &a) && !exists(&t, &b));
}
#[test]
fn malformed_settings_backups_and_missing_settings_suppress_all_deletion() {
    let t = Temp::new();
    let (id, _) = seed(&t);
    sweep(&t, now());
    let temporary = t.file(&format!("{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&temporary, b"partial").unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&temporary)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(now() - Duration::from_secs(2 * 86400)))
        .unwrap();
    let state = fs::read(t.file("gc-state.json")).unwrap();
    fs::write(t.0.join("settings.json"), b"broken").unwrap();
    {
        let _gen = GENERATION.lock().unwrap();
        assert!(cleanup_at(&t.0, day(8)).is_err());
    }
    assert!(exists(&t, &id) && temporary.exists());
    assert_eq!(fs::read(t.file("gc-state.json")).unwrap(), state);
    t.settings(&doc(&[]));
    fs::write(t.0.join("backups/unknown.json"), b"broken").unwrap();
    {
        let _gen = GENERATION.lock().unwrap();
        assert!(cleanup_at(&t.0, day(8)).is_err());
    }
    assert!(exists(&t, &id) && temporary.exists());
    fs::remove_file(t.0.join("backups/unknown.json")).unwrap();
    fs::remove_file(t.0.join("settings.json")).unwrap();
    {
        let _gen = GENERATION.lock().unwrap();
        assert!(cleanup_at(&t.0, day(8)).is_err());
    }
    assert!(exists(&t, &id) && temporary.exists());
}
#[test]
fn interrupted_files_and_incomplete_pairs_are_reclaimed_but_unknown_files_remain() {
    let t = Temp::new();
    let (a, _) = seed(&t);
    let (b, _) = seed(&t);
    fs::remove_file(path(&t.0, &a, "json").unwrap()).unwrap();
    fs::remove_file(path(&t.0, &b, "wav").unwrap()).unwrap();
    let old = t.file(&format!("{}.tmp", uuid::Uuid::new_v4()));
    let fresh = t.file(&format!("{}.tmp", uuid::Uuid::new_v4()));
    for p in [&old, &fresh] {
        fs::write(p, b"partial").unwrap();
    }
    fs::OpenOptions::new()
        .write(true)
        .open(&old)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(now() - Duration::from_secs(86401)))
        .unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&fresh)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(now()))
        .unwrap();
    for name in ["notes.tmp", "notes.json", "unknown.wav"] {
        fs::write(t.file(name), b"user data").unwrap();
    }
    sweep(&t, now());
    assert!(!old.exists() && fresh.exists());
    assert!(exists(&t, &a));
    sweep(&t, day(7));
    assert!(!exists(&t, &a));
    assert!(!path(&t.0, &b, "json").unwrap().exists());
    for name in ["notes.tmp", "notes.json", "unknown.wav"] {
        assert!(t.file(name).exists());
    }
}
#[test]
fn broken_bookkeeping_restarts_full_grace_and_backward_clock_never_deletes() {
    let t = Temp::new();
    let (id, _) = seed(&t);
    sweep(&t, now());
    let mut state: Value =
        serde_json::from_slice(&fs::read(t.file("gc-state.json")).unwrap()).unwrap();
    state["unused"][&id] = json!(0);
    fs::write(t.file("gc-state.json"), serde_json::to_vec(&state).unwrap()).unwrap();
    assert!(sweep(&t, day(8)).reset);
    assert!(exists(&t, &id));
    sweep(&t, day(14));
    assert!(exists(&t, &id));
    {
        let _gen = GENERATION.lock().unwrap();
        assert!(cleanup_at(&t.0, day(13)).is_err());
    }
    assert!(exists(&t, &id));
    sweep(&t, day(15));
    assert!(!exists(&t, &id));
}
#[test]
fn bad_json_is_quarantined_and_valid_lookup_works_without_engine() {
    let t = Temp::new();
    let (id, s) = seed(&t);
    let bad = t.file(&format!("{}.json", "0".repeat(64)));
    fs::write(&bad, b"invalid json").unwrap();
    assert!(metadata(&bad).is_none());
    assert!(!bad.exists());
    let oversized = t.file(&format!("{}.json", "1".repeat(64)));
    fs::write(&oversized, vec![b' '; 20000]).unwrap();
    let reply = generate_using(&t.0, s, 0, false, &mut |_, _, _, _| {
        panic!("valid cache must not contact engine")
    })
    .unwrap();
    assert_eq!(reply["soundId"], id);
    assert_eq!(reply["cached"], true);
    assert!(metadata(&oversized).is_none());
    assert!(!oversized.exists());
    let archives = fs::read_dir(t.0.join("voicevox"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|s| s == "corrupt"))
        .count();
    assert_eq!(archives, 2);
    sweep(&t, now());
    sweep(&t, day(7));
    assert_eq!(
        fs::read_dir(t.0.join("voicevox"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|s| s == "corrupt"))
            .count(),
        0
    );
}
#[test]
fn replacement_capacity_uses_final_size_and_reserves_temporary_space() {
    let t = Temp::new();
    let name = format!("{}.wav", "a".repeat(64));
    fs::write(t.file(&name), vec![0; 900]).unwrap();
    capacity(&t.0.join("voicevox"), &[(name.clone(), 200)], 1024).unwrap();
    assert!(capacity(&t.0.join("voicevox"), &[("new.wav".into(), 200)], 1024).is_err());
    assert!(capacity(&t.0.join("voicevox"), &[(name, 200)], 100).is_err());
}
#[test]
fn repeated_cleanup_requests_coalesce_and_are_throttled() {
    let mut queue = Queue::default();
    let root = PathBuf::from("profile");
    let start = Instant::now();
    for n in 0..100 {
        enqueue(&mut queue, root.clone(), start + Duration::from_millis(n));
    }
    assert_eq!(queue.pending.len(), 1);
    assert_eq!(queue.pending[&root], start + Duration::from_secs(2));
    queue.pending.clear();
    queue.last.insert(root.clone(), start);
    enqueue(&mut queue, root.clone(), start + Duration::from_secs(3));
    assert_eq!(queue.pending[&root], start + Duration::from_secs(60));
}
