use super::*;
#[test]
fn pinned_handle_is_hash_checked_decoded_and_read_only_until_drop() {
    let (service, sound, blob) = Service::test_registered(crate::builtin_audio::ENTRIES[0].bytes);
    let mut pin = service.pin_for_playback(&sound.id).unwrap();
    assert_eq!(service.pin_count().unwrap(), 1);
    assert!(OpenOptions::new().write(true).open(&blob).is_err());
    assert!(fs::remove_file(&blob).is_err());
    let pcm = pin.decode(|| false).unwrap();
    assert_eq!(
        pcm.samples.len() as u64 / pcm.channels as u64,
        sound.sample_frames
    );
    drop(pin);
    assert_eq!(service.pin_count().unwrap(), 0);
    assert!(OpenOptions::new().write(true).open(blob).is_ok());
}
#[test]
fn sha_valid_but_truncated_mp3_is_decode_failure_and_pin_drops() {
    let (service, mut sound, _) = Service::test_registered(crate::builtin_audio::ENTRIES[0].bytes);
    let bytes =
        &crate::builtin_audio::ENTRIES[0].bytes[..crate::builtin_audio::ENTRIES[0].bytes.len() - 1];
    sound.sha256 = sha(bytes);
    sound.byte_length = bytes.len() as u64;
    let slot = service.store.lock().unwrap();
    let store = slot.as_ref().unwrap();
    let path = store.blob(&sound.sha256).unwrap();
    paths::write_new(&path, bytes).unwrap();
    *store
        .pins
        .lock()
        .unwrap()
        .entry(sound.sha256.clone())
        .or_default() += 1;
    let mut pin = Pin {
        hash: sound.sha256.clone(),
        sound,
        pins: store.pins.clone(),
        file: Some(paths::open_read(&path).unwrap()),
    };
    drop(slot);
    assert!(pin
        .decode(|| false)
        .err()
        .expect("must fail")
        .code
        .starts_with("DecodeError:"));
    drop(pin);
    assert_eq!(service.pin_count().unwrap(), 0);
}
#[test]
fn backup_restore_cannot_implicitly_unregister_a_pinned_sound() {
    let (service, sound, _) = Service::test_registered(crate::builtin_audio::ENTRIES[0].bytes);
    {
        let mut slot = service.store.lock().unwrap();
        let store = slot.as_mut().unwrap();
        store.unregister(&sound.id, 1).unwrap();
        let source = store.root.parent().unwrap().join("selected.mp3");
        let newer = store
            .register(paths::SelectedMp3::test(source).unwrap(), "new", 2)
            .unwrap();
        let empty = fs::read_dir(store.root.join("backups"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .find(|n| n.starts_with("catalog.2."))
            .unwrap();
        let pin = store.pin(&newer.id).unwrap();
        assert_eq!(store.restore(&empty, 3).unwrap_err().code, "SoundPinned");
        drop(pin);
        store.restore(&empty, 3).unwrap();
        assert!(store.catalog.sounds.is_empty());
    }
}
