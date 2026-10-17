use super::*;
const A: &str = "builtin:church-bell";
const B: &str = "builtin:pendulum-clock";
const C: &str = "builtin:school-bell";
fn click(a: &Audio, id: &str) -> u64 {
    a.inner.lock().unwrap().preview_click = None;
    a.toggle_preview(id, 1, 0.37).unwrap()
}
fn held() -> (Arc<Audio>, Arc<Fake>) {
    let f = Fake::new(vec![Step::Stuck, Step::Normal]);
    let a = Arc::new(Audio::with_backend(f.clone(), Duration::from_secs(2)));
    a.start(|| {}).unwrap();
    until(|| a.snapshot().state == Phase::Ready);
    (a, f)
}
#[test]
fn latest_preview_replaces_waiting_b_without_overlapping_workers() {
    let (a, f) = held();
    let first = click(&a, A);
    until(|| f.live.load(Ordering::SeqCst) == 1);
    let b = click(&a, B);
    thread::sleep(Duration::from_millis(30));
    let c = click(&a, C);
    assert!(a.receipt(first).unwrap().outcome.is_none());
    assert_eq!(f.max.load(Ordering::SeqCst), 1);
    f.release.store(true, Ordering::SeqCst);
    assert!(matches!(outcome(&a, first), Outcome::Interrupted(_)));
    assert!(matches!(outcome(&a, b), Outcome::Skipped(_)));
    assert_eq!(outcome(&a, c), Outcome::Completed);
    let trace = f.trace.lock().unwrap();
    assert!(trace.iter().all(|x| x.0 == c));
    drop(trace);
    assert_eq!(a.snapshot().preview.unwrap().id, c);
    assert_eq!(f.max.load(Ordering::SeqCst), 1);
    close(&a);
}
#[test]
fn same_sound_stop_waits_for_real_acknowledgement() {
    let (a, f) = held();
    let first = click(&a, A);
    until(|| f.live.load(Ordering::SeqCst) == 1);
    let stop = click(&a, A);
    thread::sleep(Duration::from_millis(40));
    assert_eq!(a.snapshot().preview.as_ref().unwrap().state, "Stopping");
    assert!(a.receipt(first).unwrap().outcome.is_none());
    f.release.store(true, Ordering::SeqCst);
    assert_eq!(
        outcome(&a, first),
        Outcome::Interrupted("PreviewStopped".into())
    );
    until(|| a.snapshot().preview.as_ref().unwrap().state == "Stopped");
    assert_eq!(a.snapshot().preview.unwrap().id, stop);
    close(&a);
}
#[test]
fn preview_cooldown_is_shared_and_does_not_mutate_on_rejection() {
    let (a, f) = held();
    let first = a.toggle_preview(A, 1, 0.37).unwrap();
    assert_eq!(a.toggle_preview(B, 1, 0.37).unwrap_err(), "PreviewCooldown");
    assert_eq!(a.snapshot().preview.unwrap().id, first);
    thread::sleep(Duration::from_millis(310));
    assert!(a.toggle_preview(B, 1, 0.37).is_ok());
    f.release.store(true, Ordering::SeqCst);
    close(&a);
}
#[test]
fn automatic_discards_waiting_preview_and_remains_exclusive() {
    let (a, f) = held();
    let first = click(&a, A);
    until(|| f.live.load(Ordering::SeqCst) == 1);
    let waiting = click(&a, B);
    let chime = submit(&a, true, 1, "preview-priority");
    until(|| a.receipt(waiting).unwrap().outcome.is_some());
    assert_eq!(
        a.toggle_preview(C, 1, 0.37).unwrap_err(),
        "AutomaticPriority"
    );
    f.release.store(true, Ordering::SeqCst);
    assert!(matches!(outcome(&a, first), Outcome::Interrupted(_)));
    assert_eq!(outcome(&a, chime), Outcome::Completed);
    assert_eq!(f.max.load(Ordering::SeqCst), 1);
    close(&a);
}
#[test]
fn preview_stuck_keeps_custom_pin_and_never_launches_waiting_sound() {
    let (a, b, l, s, _) = custom_boot();
    a.inject(8);
    let first = click(&a, &s.id);
    until(|| l.pin_count().ok() == Some(1));
    let waiting = click(&a, B);
    until(|| a.snapshot().state == Phase::Stuck);
    assert!(a.receipt(first).unwrap().outcome.is_none());
    assert_eq!(l.pin_count().unwrap(), 1);
    assert!(matches!(outcome(&a, waiting), Outcome::Skipped(_)));
    assert_eq!(click_error(&a, C), "AudioStuck");
    let id = s.id.clone();
    assert_eq!(
        l.execute(move |store| store.unregister(&id, 1))
            .unwrap()
            .recv()
            .unwrap()
            .unwrap_err()
            .code,
        "SoundPinned"
    );
    a.inject(9);
    assert!(matches!(outcome(&a, first), Outcome::Interrupted(_)));
    assert_eq!(l.pin_count().unwrap(), 0);
    assert_eq!(b.max.load(Ordering::SeqCst), 1);
    assert_eq!(a.snapshot().preview.unwrap().state, "Skipped");
    close(&a);
}
fn click_error(a: &Audio, id: &str) -> String {
    a.inner.lock().unwrap().preview_click = None;
    a.toggle_preview(id, 1, 0.37).unwrap_err()
}
#[test]
fn failed_preview_releases_pin_and_next_request_prepares_again() {
    let (a, _, l, s, _) = custom_boot();
    a.inject(7);
    let id = click(&a, &s.id);
    assert!(matches!(outcome(&a, id), Outcome::Failed(_)));
    assert_eq!(l.pin_count().unwrap(), 0);
    let next = click(&a, &s.id);
    assert_eq!(outcome(&a, next), Outcome::Completed);
    assert_eq!(a.snapshot().preview.unwrap().id, next);
    close(&a);
}
