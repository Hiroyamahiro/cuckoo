use super::*;
#[test]
fn filename_registration_and_rename_preserve_identity_and_recover() {
    let root = fixture();
    let file = root.join("選んだ音源.mp3");
    fs::write(&file, original()).unwrap();
    let selected = paths::SelectedMp3::test(file.clone()).unwrap();
    assert_eq!(selected.registration_name().unwrap(), "選んだ音源");
    let mut store = open(&root);
    let sound = store.register(selected, "選んだ音源.mp3", 0).unwrap();
    let bytes = fs::read(store.blob(&sound.sha256).unwrap()).unwrap();
    assert!(store.rename(&sound.id, "", 1).is_err());
    assert!(store.rename("user:invalid", "名前", 1).is_err());
    let pin = store.pin(&sound.id).unwrap();
    store.rename(&sound.id, "変更後", 1).unwrap();
    assert_eq!(store.catalog.sounds[0].id, sound.id);
    assert_eq!(store.catalog.sounds[0].sha256, sound.sha256);
    assert_eq!(fs::read(store.blob(&sound.sha256).unwrap()).unwrap(), bytes);
    assert_eq!(store.unregister(&sound.id, 2).unwrap_err().code, "SoundPinned");
    drop(pin);
    let duplicate = store.register(paths::SelectedMp3::test(file).unwrap(), "別名", 2).unwrap();
    assert_eq!(duplicate.id, sound.id);
    assert_eq!(duplicate.name, "変更後");
    store.fault=Some(Stage::BeforeReplace);
    assert!(store.rename(&sound.id,"失敗した名前",2).is_err());
    drop(store);
    let mut store=open(&root);
    assert_eq!(store.snapshot().unwrap().sounds[0].name,"変更後");
    store.fault=Some(Stage::AfterReplace);
    assert!(store.rename(&sound.id,"確定後",2).unwrap_err().uncertain);
    assert!(store.rename(&sound.id,"再操作",2).is_err());
    drop(store);
    let store=open(&root);
    assert_eq!(store.snapshot().unwrap().sounds[0].name,"確定後");
}
fn fixture() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("test-runtime/library")
        .join(Uuid::new_v4().to_string());
    fs::create_dir_all(&root).unwrap();
    root
}
fn source(root: &Path, bytes: &[u8]) -> paths::SelectedMp3 {
    let path = root.join(format!("{}.mp3", Uuid::new_v4()));
    fs::write(&path, bytes).unwrap();
    paths::SelectedMp3::test(path).unwrap()
}
fn original() -> &'static [u8] {
    crate::builtin_audio::ENTRIES[0].bytes
}
fn small_mp3(tag: u8, frames: usize) -> Vec<u8> {
    let mut b = b"ID3\x03\0\0\0\0\0\x20".to_vec();
    b.extend([0u8; 32]);
    b[41] = tag; // Legal ID3 padding; preserve a zero padding marker at its start.
    for _ in 0..frames {
        let mut frame = vec![0; 384];
        frame[..4].copy_from_slice(&[255, 251, 148, 0]);
        b.extend(frame);
    }
    b
}
fn open(root: &Path) -> Store {
    Store::open(root.join("sounds")).unwrap()
}
#[test]
fn limits_exact_bytes_samples_count_and_capacity() {
    for rate in [44100u32, 48000] {
        let min = (rate as u64).div_ceil(1000);
        assert!(quota(1, min, rate, 0, 0).is_ok());
        assert_eq!(
            quota(1, min - 1, rate, 0, 0).unwrap_err().code,
            "DurationLimit"
        );
        assert!(quota(MAX_FILE, rate as u64 * 60, rate, 99, MAX_TOTAL - MAX_FILE).is_ok());
        assert_eq!(
            quota(MAX_FILE + 1, rate as u64, rate, 0, 0)
                .unwrap_err()
                .code,
            "FileSizeLimit"
        );
        assert_eq!(
            quota(1, rate as u64 * 60 + 1, rate, 0, 0).unwrap_err().code,
            "DurationLimit"
        );
    }
    assert!(quota(0, 44100, 44100, 0, 0).is_err());
    assert_eq!(
        quota(1, 44100, 44100, 100, 0).unwrap_err().code,
        "CountLimit"
    );
    assert!(quota(1, 44100, 44100, 99, MAX_TOTAL - 1).is_ok());
    assert_eq!(
        quota(1, 44100, 44100, 0, MAX_TOTAL).unwrap_err().code,
        "CapacityLimit"
    );
    assert!(quota(1, u64::MAX, 44100, 0, 0).is_err());
    assert!(quota(1, 44100, 44100, 0, u64::MAX).is_err());
}
#[test]
fn register_duplicate_delete_source_restart_pin_unregister_restore() {
    let root = fixture();
    let mut store = open(&root);
    let selected = source(&root, original());
    let original_path = selected.path.clone();
    let s = store.register(selected, "  任意の鳩  ", 0).unwrap();
    assert_eq!(s.name, "任意の鳩");
    assert!(id_valid(&s.id));
    assert_eq!(s.sha256, sha(original()));
    fs::remove_file(original_path).unwrap();
    assert_eq!(
        store
            .register(source(&root, original()), "別名", 1)
            .unwrap()
            .id,
        s.id
    );
    assert_eq!(store.snapshot().unwrap().generation, 1);
    assert_eq!(
        store.unregister(&s.id, 0).unwrap_err().code,
        "StaleGeneration"
    );
    let pin = store.pin(&s.id).unwrap();
    assert_eq!(store.unregister(&s.id, 1).unwrap_err().code, "SoundPinned");
    drop(pin);
    let blob = store.blob(&s.sha256).unwrap();
    store.unregister(&s.id, 1).unwrap();
    assert!(store.catalog.sounds.is_empty());
    assert!(blob.exists());
    let backup = fs::read_dir(store.root.join("backups"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .find(|n| n.starts_with("catalog.1."))
        .unwrap();
    store.restore(&backup, 2).unwrap();
    assert_eq!(store.catalog.sounds[0].id, s.id);
    drop(store);
    let restored = open(&root);
    assert_eq!(restored.catalog.generation, 3);
    assert_eq!(restored.catalog.sounds[0].id, s.id);
    restored.verify_sound(&s).unwrap();
}
#[test]
fn invalid_format_corrupt_mp3_and_name_never_publish() {
    let root = fixture();
    let mut s = open(&root);
    for bytes in [
        b"not an mp3".to_vec(),
        original()[..original().len() - 1].to_vec(),
    ] {
        assert!(s.register(source(&root, &bytes), "bad", 0).is_err());
        assert_eq!(s.snapshot().unwrap().generation, 0);
    }
    for name in ["", "\n", "a\0b", &"a".repeat(81)] {
        assert!(s.register(source(&root, original()), name, 0).is_err());
    }
    assert!(!s.root.join("catalog.json").exists());
    assert!(Catalog {
        schema_version: 2,
        ..Default::default()
    }
    .validate()
    .is_err());
}
#[test]
fn actual_25mib_and_60_second_files_and_overlimits() {
    let root = fixture();
    let base = small_mp3(1, 3);
    let payload = &base[42..];
    let size = MAX_FILE as usize;
    let tag_len = size - 10 - payload.len();
    let mut padded = vec![0u8; size];
    padded[..6].copy_from_slice(b"ID3\x03\0\0");
    for i in 0..4 {
        padded[6 + i] = ((tag_len >> ((3 - i) * 7)) & 127) as u8;
    }
    padded[10 + tag_len..].copy_from_slice(payload);
    let mut s = open(&root);
    let exact = s.register(source(&root, &padded), "25MiB", 0).unwrap();
    assert_eq!(exact.byte_length, MAX_FILE);
    let path = root.join("oversize.mp3");
    let f = File::create(&path).unwrap();
    f.set_len(MAX_FILE + 1).unwrap();
    drop(f);
    assert!(paths::SelectedMp3::test(path).is_err());
    let exact60 = s
        .register(source(&root, &small_mp3(2, 2500)), "60 seconds", 1)
        .unwrap();
    assert_eq!(exact60.sample_rate, 48000);
    assert_eq!(exact60.sample_frames, 48000 * 60);
    assert_eq!(exact60.duration_ms, 60000);
    assert!(s
        .register(source(&root, &small_mp3(3, 2501)), "over60", 2)
        .is_err());
    assert_eq!(s.catalog.sounds.len(), 2);
}
#[test]
fn actual_100_entries_duplicate_at_limit_and_101_rejected() {
    let root = fixture();
    let mut s = open(&root);
    for i in 0..100 {
        let entry = s
            .register(source(&root, &small_mp3(i as u8, 3)), "同名", i)
            .unwrap();
        assert!(id_valid(&entry.id));
    }
    assert_eq!(s.catalog.sounds.len(), 100);
    assert_eq!(
        s.register(source(&root, &small_mp3(100, 3)), "101", 100)
            .unwrap_err()
            .code,
        "CountLimit"
    );
    assert_eq!(
        s.register(source(&root, &small_mp3(0, 3)), "duplicate", 100)
            .unwrap()
            .id,
        s.catalog.sounds[0].id
    );
    drop(s);
    assert_eq!(open(&root).catalog.sounds.len(), 100);
}
#[test]
fn failures_before_publish_retain_files_and_retry_safely() {
    for stage in [
        Stage::AfterValidation,
        Stage::AfterCopy,
        Stage::AfterJournal,
        Stage::AfterBlob,
        Stage::Temp,
        Stage::BeforeReplace,
    ] {
        let root = fixture();
        let mut s = open(&root);
        s.fault = Some(stage);
        let e = s
            .register(source(&root, original()), "candidate", 0)
            .unwrap_err();
        assert!(!e.uncertain);
        assert_eq!(s.catalog.generation, 0);
        s.fault = None;
        drop(s);
        let mut recovered = open(&root);
        assert_eq!(recovered.catalog.generation, 0);
        recovered
            .register(source(&root, &small_mp3(8, 3)), "retry different", 0)
            .unwrap();
        drop(recovered);
        assert_eq!(open(&root).catalog.generation, 1);
    }
}
#[test]
fn uncertain_commit_blocks_save_delete_and_only_reconciles_actual_disk() {
    for stage in [Stage::AfterReplace, Stage::FinalVerify] {
        let root = fixture();
        let mut s = open(&root);
        s.fault = Some(stage);
        let e = s
            .register(source(&root, original()), "uncertain", 0)
            .unwrap_err();
        assert!(e.uncertain);
        assert_eq!(s.catalog.generation, 0);
        assert!(s.snapshot().unwrap_err().uncertain);
        assert!(
            s.register(source(&root, original()), "retry", 0)
                .unwrap_err()
                .uncertain
        );
        assert!(
            s.unregister("user:00000000-0000-4000-8000-000000000000", 0)
                .unwrap_err()
                .uncertain
        );
        s.fault = None;
        let r = s.reconcile().unwrap();
        assert_eq!(r.committed, 1);
        assert_eq!(s.catalog.generation, 1);
        assert_eq!(s.catalog.sounds.len(), 1);
        drop(s);
        assert_eq!(open(&root).catalog.sounds.len(), 1);
    }
}
#[test]
fn backup_failure_and_unregister_uncertainty_preserve_blob_and_prior_backup() {
    let root = fixture();
    let mut s = open(&root);
    let item = s.register(source(&root, original()), "first", 0).unwrap();
    s.fault = Some(Stage::Backup);
    assert!(s.unregister(&item.id, 1).is_err());
    assert_eq!(s.catalog.sounds.len(), 1);
    s.fault = Some(Stage::AfterReplace);
    assert!(s.unregister(&item.id, 1).unwrap_err().uncertain);
    assert!(s.blob(&item.sha256).unwrap().exists());
    s.fault = None;
    s.reconcile().unwrap();
    assert!(s.catalog.sounds.is_empty());
    let backup = fs::read_dir(s.root.join("backups"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .file_name()
        .to_string_lossy()
        .into_owned();
    s.restore(&backup, 2).unwrap();
    assert_eq!(s.catalog.sounds[0].id, item.id);
}
#[test]
fn invalid_paths_hardlinks_managed_sources_and_backup_tokens_rejected() {
    for p in [
        "relative.mp3",
        "C:\\dummy\\..\\a.mp3",
        "C:\\dummy\\.\\a.mp3",
        "\\\\host\\share\\x.mp3",
        "\\\\?\\C:\\x.mp3",
        "C:\\x.mp3:stream",
        "C:\\x.mp3.",
        "C:\\NUL.mp3",
    ] {
        assert!(paths::lexical(Path::new(p)).is_err(), "{p}");
    }
    let root = fixture();
    let one = root.join("one.mp3");
    let two = root.join("two.mp3");
    fs::write(&one, original()).unwrap();
    fs::hard_link(&one, &two).unwrap();
    assert!(paths::SelectedMp3::test(one).is_err());
    let mut s = open(&root);
    let item = s.register(source(&root, original()), "first", 0).unwrap();
    let blob = s.blob(&item.sha256).unwrap();
    let slash = PathBuf::from(blob.to_string_lossy().replace('\\', "/"));
    assert_eq!(
        s.register(paths::SelectedMp3::test(slash).unwrap(), "managed slash", 1)
            .unwrap_err()
            .code,
        "ManagedSourceRejected"
    );
    assert_eq!(
        s.register(paths::SelectedMp3::test(blob).unwrap(), "managed", 1)
            .unwrap_err()
            .code,
        "ManagedSourceRejected"
    );
    for token in ["../catalog.json", "C:\\other.json", "catalog.0...json"] {
        assert!(s.restore(token, 1).is_err());
    }
    assert!(Store::open(root.join("sounds")).is_err());
}
#[test]
fn junction_ancestor_is_rejected_without_touching_target() {
    let root = fixture();
    let target = root.join("outside");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("chosen.mp3"), original()).unwrap();
    let link = root.join("junction");
    let status = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(link.to_string_lossy().replace('/', "\\"))
        .arg(target.to_string_lossy().replace('/', "\\"))
        .output()
        .unwrap();
    assert!(status.status.success(), "{:?}", status);
    assert!(paths::SelectedMp3::test(link.join("chosen.mp3")).is_err());
    assert!(Store::open(link.join("sounds")).is_err());
    assert!(!target.join("sounds").exists());
    assert_eq!(fs::read(target.join("chosen.mp3")).unwrap(), original());
}
#[test]
fn corrupted_catalog_journal_and_missing_blob_are_read_blocked() {
    for case in ["catalog", "journal", "missing", "future", "changed"] {
        let root = fixture();
        let mut s = open(&root);
        let item = s.register(source(&root, original()), "first", 0).unwrap();
        let catalog = s.root.join("catalog.json");
        let blob = s.blob(&item.sha256).unwrap();
        let journal = fs::read_dir(s.root.join("transactions"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        drop(s);
        match case {
            "catalog" => fs::write(&catalog, b"{").unwrap(),
            "journal" => fs::write(journal, b"{").unwrap(),
            "missing" => fs::remove_file(blob).unwrap(),
            "future" => {
                let mut c: Catalog = serde_json::from_slice(&fs::read(&catalog).unwrap()).unwrap();
                c.schema_version = 2;
                fs::write(&catalog, serde_json::to_vec(&c).unwrap()).unwrap()
            }
            _ => fs::write(blob, b"changed").unwrap(),
        }
        let before = fs::read(&catalog).unwrap();
        if ["missing", "changed"].contains(&case) {
            let opened = Store::open(root.join("sounds")).unwrap();
            assert!(opened
                .pin(&item.id)
                .and_then(|mut p| p.decode(|| false).map(|_| ()))
                .is_err());
        } else {
            assert!(Store::open(root.join("sounds")).is_err());
        }
        assert_eq!(fs::read(catalog).unwrap(), before);
    }
}
#[test]
fn external_catalog_change_blocks_operations_without_rewrite() {
    let root = fixture();
    let mut s = open(&root);
    s.register(source(&root, original()), "first", 0).unwrap();
    let path = s.root.join("catalog.json");
    fs::write(&path, b"external").unwrap();
    assert!(
        s.register(source(&root, original()), "again", 1)
            .unwrap_err()
            .uncertain
    );
    assert_eq!(fs::read(path).unwrap(), b"external");
}
#[test]
fn crash_child() {
    let Ok(root) = std::env::var("CUCKOO_D1_CRASH_FIXTURE") else {
        return;
    };
    let root = PathBuf::from(root);
    assert!(root.starts_with(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("test-runtime/library")
    ));
    let mut s = open(&root);
    s.crash = Some(
        match std::env::var("CUCKOO_D1_CRASH_STAGE").unwrap().as_str() {
            "copy" => Stage::AfterCopy,
            "blob" => Stage::AfterBlob,
            _ => Stage::AfterReplace,
        },
    );
    s.register(source(&root, original()), "child", 0).unwrap();
    panic!("crash injection did not execute");
}
#[test]
fn real_process_exit_during_registration_recovers_without_auto_promoting() {
    for stage in ["copy", "blob", "replace"] {
        let root = fixture();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "sound_library::tests::crash_child",
                "--nocapture",
            ])
            .env("CUCKOO_D1_CRASH_FIXTURE", &root)
            .env("CUCKOO_D1_CRASH_STAGE", stage)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(73));
        let s = open(&root);
        if stage == "replace" {
            assert_eq!(s.catalog.sounds.len(), 1);
            assert_eq!(s.recovery.committed, 1)
        } else {
            assert!(s.catalog.sounds.is_empty());
            if stage == "copy" {
                assert_eq!(s.recovery.retained_staging, 1)
            } else {
                assert_eq!(s.recovery.retained_orphan_blobs, 1)
            }
        }
    }
}

#[test]
fn service_is_single_flight_survives_handle_drop_and_poison_is_not_success() {
    let root = fixture();
    let service = Service {
        settings: Arc::default(),
        store: Arc::new(Mutex::new(Some(open(&root)))),
        busy: Arc::new(AtomicBool::new(false)),
    };
    let (entered, rx_entered) = mpsc::sync_channel(1);
    let (release, rx_release) = mpsc::sync_channel(1);
    let result = service
        .execute(move |store| {
            entered.send(()).unwrap();
            rx_release.recv_timeout(Duration::from_secs(10)).unwrap();
            store.snapshot()
        })
        .unwrap();
    rx_entered.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(
        service.execute(|s| s.snapshot()).err().unwrap().code,
        "LibraryBusy"
    );
    let other = service.clone();
    drop(service);
    release.send(()).unwrap();
    assert_eq!(
        result
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .unwrap()
            .generation,
        0
    );
    while other.busy.load(Ordering::SeqCst) {
        thread::yield_now();
    }
    let failed = other
        .execute::<()>(|_| panic!("injected library worker panic"))
        .unwrap();
    assert!(
        failed
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .unwrap_err()
            .uncertain
    );
    while other.busy.load(Ordering::SeqCst) {
        thread::yield_now();
    }
    let after = other.execute(|s| s.snapshot()).unwrap();
    assert_eq!(
        after
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .unwrap_err()
            .code,
        "LibraryPoisoned"
    );
}
#[test]
fn retained_blob_and_staging_count_toward_quota_and_backup_metadata_is_not_audio() {
    let root = fixture();
    let mut s = open(&root);
    let item = s.register(source(&root, original()), "first", 0).unwrap();
    assert_eq!(s.used().unwrap(), original().len() as u64);
    s.unregister(&item.id, 1).unwrap();
    assert_eq!(s.used().unwrap(), original().len() as u64);
    let stage = s
        .root
        .join("staging")
        .join(format!("{}.part", Uuid::new_v4()));
    paths::write_new(&stage, &[0; 17]).unwrap();
    assert_eq!(s.used().unwrap(), original().len() as u64 + 17);
    let used = s.used().unwrap();
    assert!(quota(1, 48000, 48000, 0, MAX_TOTAL - 1).is_ok());
    assert_eq!(
        quota(2, 48000, 48000, 0, MAX_TOTAL - 1).unwrap_err().code,
        "CapacityLimit"
    );
    assert!(used > 0);
    drop(s);
    let s = open(&root);
    assert_eq!(s.recovery.retained_staging, 1);
    assert_eq!(s.recovery.retained_orphan_blobs, 1);
}

#[test]
fn network_unknown_and_missing_drive_types_are_rejected() {
    for kind in [0, 1, 4] {
        assert_eq!(
            paths::test_drive(kind).unwrap_err().code,
            "LocalDriveRequired"
        );
    }
    for kind in [2, 3, 5, 6] {
        assert!(paths::test_drive(kind).is_ok());
    }
}
