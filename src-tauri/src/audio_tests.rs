use super::*;
#[path = "audio_preview_tests.rs"]
mod preview_tests;
use std::sync::atomic::{AtomicUsize, Ordering};
#[derive(Clone)]
enum Step {
    Normal,
    LongTest,
    Preparing,
    DeviceError,
    DecodeError,
    Stuck,
    Panic,
    NaturalLateCompletion,
}
struct Fake {
    steps: Mutex<VecDeque<Step>>,
    live: AtomicUsize,
    max: AtomicUsize,
    release: AtomicBool,
    trace: Mutex<Vec<(u64, u64, Instant)>>,
}
impl Fake {
    fn new(steps: Vec<Step>) -> Arc<Self> {
        Arc::new(Self {
            steps: Mutex::new(steps.into()),
            live: AtomicUsize::new(0),
            max: AtomicUsize::new(0),
            release: AtomicBool::new(false),
            trace: Mutex::new(vec![]),
        })
    }
}
struct Guard<'a>(&'a AtomicUsize);
impl Drop for Guard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
impl Backend for Fake {
    fn run(&self, r: &Request, c: &Cancel, events: &SyncSender<Event>) -> Outcome {
        if r.kind == Kind::Probe {
            return Outcome::Completed;
        }
        let n = self.live.fetch_add(1, Ordering::SeqCst) + 1;
        self.max.fetch_max(n, Ordering::SeqCst);
        let _guard = Guard(&self.live);
        let step = self
            .steps
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Step::Normal);
        match step {
            Step::DeviceError => return Outcome::Failed("DeviceOpenError:Injected".into()),
            Step::DecodeError => return Outcome::Failed("DecodeError:Injected".into()),
            Step::Panic => panic!("Injected worker panic"),
            Step::NaturalLateCompletion => {
                while !self.release.load(Ordering::SeqCst) {
                    thread::sleep(POLL);
                }
                return Outcome::Completed;
            }
            Step::Stuck => {
                while !self.release.load(Ordering::SeqCst) {
                    thread::sleep(POLL)
                }
                return c.outcome().unwrap_or(Outcome::Completed);
            }
            Step::Preparing => {
                while c.outcome().is_none() {
                    thread::sleep(POLL)
                }
                return c.outcome().unwrap();
            }
            _ => (),
        }
        let _ = events.try_send(Event::Prepared(Duration::from_millis(20)));
        for k in 1..=r.repeat {
            self.trace.lock().unwrap().push((r.id, k, Instant::now()));
            let _ = events.try_send(Event::Started(k));
            let started = Instant::now();
            while started.elapsed() < Duration::from_millis(30) || matches!(step, Step::LongTest) {
                if let Some(o) = c.outcome() {
                    return o;
                }
                thread::sleep(POLL)
            }
            if k < r.repeat {
                let _ = events.try_send(Event::Gap);
                let start = Instant::now();
                while start.elapsed() < GAP {
                    if let Some(o) = c.outcome() {
                        return o;
                    }
                    thread::sleep(POLL)
                }
            }
        }
        Outcome::Completed
    }
}
fn until(mut test: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(3);
    while !test() {
        assert!(Instant::now() < end, "condition deadline");
        thread::sleep(POLL)
    }
}
fn boot(steps: Vec<Step>) -> (Arc<Audio>, Arc<Fake>) {
    let fake = Fake::new(steps);
    let audio = Arc::new(Audio::with_backend(fake.clone(), Duration::from_millis(30)));
    audio.start(|| {}).unwrap();
    until(|| audio.snapshot().state == Phase::Ready);
    (audio, fake)
}
fn submit(a: &Audio, automatic: bool, repeat: u64, key: &str) -> u64 {
    let end = Instant::now() + Duration::from_secs(1);
    loop {
        match a.submit(
            automatic,
            repeat,
            0.37,
            automatic.then_some(key),
            automatic.then(|| Instant::now() + Duration::from_secs(2)),
        ) {
            Ok(id) => return id,
            Err(e) if e == "AudioMailboxBusy" => {
                assert!(Instant::now() < end);
                thread::sleep(POLL)
            }
            Err(e) => panic!("{e}"),
        }
    }
}
fn outcome(a: &Audio, id: u64) -> Outcome {
    until(|| a.receipt(id).unwrap().outcome.is_some());
    a.receipt(id).unwrap().outcome.unwrap()
}
fn accepted(mut submit: impl FnMut() -> Result<u64, String>) -> u64 {
    let end = Instant::now() + Duration::from_secs(1);
    loop {
        match submit() {
            Ok(id) => return id,
            Err(e) if e == "AudioMailboxBusy" => { assert!(Instant::now() < end); thread::sleep(POLL); }
            Err(e) => panic!("{e}"),
        }
    }
}
fn close(a: &Audio) {
    assert!(a.shutdown(Duration::from_secs(1)))
}
#[test]
fn normal_test_and_two_iterations_keep_250ms_gap() {
    let (a, f) = boot(vec![Step::Normal]);
    let id = submit(&a, false, 2, "");
    assert_eq!(outcome(&a, id), Outcome::Completed);
    let t = f.trace.lock().unwrap();
    assert_eq!(t.len(), 2);
    assert!(t[1].2.duration_since(t[0].2) >= GAP + Duration::from_millis(30));
    assert_eq!(a.snapshot().state, Phase::Ready);
    drop(t);
    close(&a);
}
#[test]
fn regular_and_custom_chimes_at_same_minute_complete_sequentially() {
    let (audio, fake) = boot(vec![Step::Normal, Step::Normal]);
    let settings = serde_json::json!({"enabled":true,"slots":vec![true;48],"volume":50,
        "hourlyRepeat":1,"audioRouting":{"onHour":{"soundId":crate::builtin_audio::DEFAULT_ID,"volume":60}},
        "customChimes":[{"id":"custom","minute":540,"enabled":true,"soundId":crate::builtin_audio::DEFAULT_ID,"repeat":2,"volume":80}]});
    let mut ids = vec![];
    for event in crate::scheduler::due(540, &settings) {
        if let Some(previous) = ids.last() { assert_eq!(outcome(&audio, *previous), Outcome::Completed); }
        let id = accepted(|| audio.submit_builtin(true, event.repeat, event.volume, Some(&event.key), Some(Instant::now()+Duration::from_secs(2)), &event.sound_id));
        ids.push(id);
    }
    assert_eq!(outcome(&audio, ids[1]), Outcome::Completed);
    let trace = fake.trace.lock().unwrap();
    assert_eq!(trace.iter().map(|t| t.0).collect::<Vec<_>>(), vec![ids[0],ids[1],ids[1]]);
    assert_eq!(fake.max.load(Ordering::SeqCst), 1);
    drop(trace); close(&audio);
}
#[test]
fn scheduled_zero_repeat_one_two_and_half_hour_one() {
    let (a, f) = boot(vec![]);
    for (minute, repeat, key) in [(0, 1, "a"), (0, 2, "b"), (30, 2, "c")] {
        let n = crate::scheduler::repeat_for(minute, &serde_json::json!({"hourlyRepeat":repeat}));
        let id = submit(&a, true, n, key);
        assert_eq!(outcome(&a, id), Outcome::Completed);
        assert_eq!(
            f.trace.lock().unwrap().iter().filter(|x| x.0 == id).count(),
            n as usize
        );
    }
    close(&a);
}
#[test]
fn automatic_preempts_long_test_without_waiting_or_overlap() {
    let (a, f) = boot(vec![Step::LongTest, Step::Normal]);
    let test = submit(&a, false, 2, "");
    until(|| a.snapshot().state == Phase::Playing);
    let started = Instant::now();
    let automatic = submit(&a, true, 1, "boundary");
    assert!(started.elapsed() < Duration::from_millis(100));
    assert_eq!(
        outcome(&a, test),
        Outcome::Interrupted("PreemptedByChime".into())
    );
    assert_eq!(outcome(&a, automatic), Outcome::Completed);
    assert_eq!(f.max.load(Ordering::SeqCst), 1);
    close(&a);
}
#[test]
fn automatic_preempts_preparation_and_repeat_gap() {
    for preparing in [true, false] {
        let (a, _) = boot(vec![
            if preparing {
                Step::Preparing
            } else {
                Step::Normal
            },
            Step::Normal,
        ]);
        let id = submit(&a, false, 2, "");
        until(|| a.receipt(id).unwrap().phase == if preparing { "Preparing" } else { "RepeatGap" });
        let auto = submit(&a, true, 1, "next");
        assert_eq!(
            outcome(&a, id),
            Outcome::Interrupted("PreemptedByChime".into())
        );
        assert_eq!(outcome(&a, auto), Outcome::Completed);
        close(&a);
    }
}
#[test]
fn device_and_decode_errors_notify_then_next_request_reprepares() {
    for (step, prefix, phase) in [
        (Step::DeviceError, "DeviceOpenError", Phase::Unavailable),
        (Step::DecodeError, "DecodeError", Phase::Failed),
    ] {
        let (a, _) = boot(vec![step, Step::Normal]);
        let id = submit(&a, false, 1, "");
        assert!(matches!(outcome(&a,id),Outcome::Failed(code) if code.starts_with(prefix)));
        assert_eq!(a.snapshot().state, phase);
        assert!(!a.snapshot().available);
        assert!(a.snapshot().can_test);
        let next = submit(&a, false, 1, "");
        assert_eq!(outcome(&a, next), Outcome::Completed);
        assert!(a.snapshot().available);
        close(&a);
    }
}
#[test]
fn stuck_does_not_create_another_worker_or_claim_interruption_complete() {
    let (a, f) = boot(vec![Step::Stuck, Step::Normal]);
    let id = submit(&a, false, 1, "");
    until(|| f.live.load(Ordering::SeqCst) == 1);
    a.cancel();
    until(|| a.snapshot().state == Phase::Stuck);
    assert_eq!(a.receipt(id).unwrap().outcome, None);
    assert_eq!(
        a.submit(false, 1, 0.5, None, None).unwrap_err(),
        "AudioStuck"
    );
    assert!(!a.shutdown(Duration::from_millis(50)));
    assert_eq!(f.max.load(Ordering::SeqCst), 1);
    f.release.store(true, Ordering::SeqCst);
    close(&a);
    assert!(matches!(
        a.receipt(id).unwrap().outcome,
        Some(Outcome::Interrupted(_))
    ));
}
#[test]
fn mutex_contention_returns_immediately_and_repeated_test_requests_are_bounded() {
    let (a, f) = boot(vec![Step::LongTest]);
    {
        let _held = a.inner.lock().unwrap();
        assert_eq!(
            a.submit(true, 1, 0.5, Some("busy"), None).unwrap_err(),
            "AudioMailboxBusy"
        );
    }
    let id = submit(&a, false, 1, "");
    until(|| a.snapshot().state == Phase::Playing);
    let mut threads = vec![];
    for _ in 0..16 {
        let audio = a.clone();
        threads.push(thread::spawn(move || {
            assert!(audio.submit(false, 1, 0.5, None, None).is_err())
        }));
    }
    for t in threads {
        t.join().unwrap()
    }
    a.cancel();
    assert!(matches!(outcome(&a, id), Outcome::Interrupted(_)));
    assert_eq!(f.max.load(Ordering::SeqCst), 1);
    close(&a);
}
#[test]
fn failed_chime_is_not_retried_and_expired_chime_is_skipped() {
    let (a, f) = boot(vec![Step::DeviceError]);
    let id = submit(&a, true, 1, "same");
    assert!(matches!(outcome(&a, id), Outcome::Failed(_)));
    // A terminal receipt does not reserve the coordinator's short mailbox lock.
    // Retry only contention; still require the real duplicate-chime rejection.
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    let duplicate = loop {
        let error = a.submit(true, 1, 0.5, Some("same"), None).unwrap_err();
        if error != "AudioMailboxBusy" {
            break error;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "mailbox did not become available"
        );
        thread::sleep(Duration::from_millis(1));
    };
    assert_eq!(duplicate, "DuplicateChime");
    let id = a
        .submit(
            true,
            1,
            0.5,
            Some("old"),
            Some(Instant::now() - Duration::from_secs(1)),
        )
        .unwrap();
    assert_eq!(outcome(&a, id), Outcome::Skipped("DeadlineExceeded".into()));
    assert_eq!(f.max.load(Ordering::SeqCst), 1);
    close(&a);
}
#[test]
fn panic_is_failed_and_shutdown_cancels_without_locking_scheduler() {
    let (a, _) = boot(vec![Step::Panic, Step::LongTest]);
    let id = submit(&a, false, 1, "");
    assert_eq!(outcome(&a, id), Outcome::Failed("WorkerPanic".into()));
    let next = submit(&a, false, 1, "");
    until(|| a.snapshot().state == Phase::Playing);
    close(&a);
    assert_eq!(
        a.receipt(next).unwrap().outcome,
        Some(Outcome::Interrupted("Shutdown".into()))
    );
    assert_eq!(a.snapshot().state, Phase::Stopped);
    assert!(a.start(|| {}).is_err());
}
#[test]
fn default_mp3_has_finite_duration() {
    let d = crate::mp3_decode::decode(crate::builtin_audio::ENTRIES[0].bytes, || false).unwrap().duration();
    assert!(d > Duration::ZERO && d < Duration::from_secs(30));
    assert_eq!(GAP, Duration::from_millis(250));
}
#[test]
fn preparation_deadline_requests_cancel_and_reports_failure_not_completion() {
    let (a, _) = boot(vec![Step::Preparing]);
    let id = accepted(|| a.submit(
            true,
            1,
            0.5,
            Some("deadline"),
            Some(Instant::now() + Duration::from_millis(40)),
        ));
    assert_eq!(outcome(&a, id), Outcome::Failed("PlaybackTimeout".into()));
    assert_eq!(a.snapshot().state, Phase::Failed);
    close(&a);
}
#[test]
fn stuck_test_keeps_worker_while_pending_chime_expires() {
    let (a, f) = boot(vec![Step::Stuck]);
    let test = submit(&a, false, 1, "");
    until(|| f.live.load(Ordering::SeqCst) == 1);
    // The non-blocking mailbox may briefly be locked by the coordinator.
    // Retry admission only; keep one deadline and never duplicate an accepted request.
    let deadline = Instant::now() + Duration::from_millis(100);
    let id = loop {
        match a.submit(true, 1, 0.5, Some("expiry"), Some(deadline)) {
            Ok(id) => break id,
            Err(e) if e == "AudioMailboxBusy" => {
                assert!(Instant::now() < deadline, "mailbox admission deadline");
                thread::sleep(POLL);
            }
            Err(e) => panic!("{e}"),
        }
    };
    until(|| a.snapshot().state == Phase::Stuck);
    assert_eq!(outcome(&a, id), Outcome::Skipped("DeadlineExceeded".into()));
    assert_eq!(a.snapshot().state, Phase::Stuck);
    assert!(a.receipt(test).unwrap().outcome.is_none());
    assert_eq!(f.max.load(Ordering::SeqCst), 1);
    f.release.store(true, Ordering::SeqCst);
    assert_eq!(
        outcome(&a, test),
        Outcome::Interrupted("PreemptedByChime".into())
    );
    close(&a);
}
#[test]
fn cancellation_request_does_not_falsely_relabel_natural_completion() {
    let (a, f) = boot(vec![Step::NaturalLateCompletion]);
    let id = submit(&a, false, 1, "");
    until(|| f.live.load(Ordering::SeqCst) == 1);
    a.cancel();
    until(|| a.snapshot().state == Phase::Stuck);
    assert!(a.receipt(id).unwrap().outcome.is_none());
    f.release.store(true, Ordering::SeqCst);
    assert_eq!(outcome(&a, id), Outcome::Completed);
    close(&a);
}

struct CustomBackend {
    real: Mp3Backend,
    hold_playing: AtomicBool,
    live: AtomicUsize,
    max: AtomicUsize,
    trace: Mutex<Vec<Instant>>,
}
impl Backend for CustomBackend {
    fn inject(&self, mode: u8) {
        self.real.inject(mode)
    }
    fn run(&self, r: &Request, c: &Cancel, events: &SyncSender<Event>) -> Outcome {
        if r.kind == Kind::Probe {
            return Outcome::Completed;
        }
        let live = self.live.fetch_add(1, Ordering::SeqCst) + 1;
        self.max.fetch_max(live, Ordering::SeqCst);
        let _guard = Guard(&self.live);
        // Exercise the real pinned preparation faults, without opening a physical device.
        if self.real.next_fault.load(Ordering::SeqCst) != 0 {
            return self.real.run(r, c, events);
        }
        let mut pin = match self.real.library.pin_for_playback(&r.sound_id) {
            Ok(p) => p,
            Err(e) => return Outcome::Failed(e.code),
        };
        let pcm = match pin.decode(|| c.outcome().is_some()) {
            Ok(p) => p,
            Err(e) => return c.outcome().unwrap_or(Outcome::Failed(e.code)),
        };
        let _ = events.try_send(Event::Prepared(pcm.duration()));
        for i in 1..=r.repeat {
            self.trace.lock().unwrap().push(Instant::now());
            let _ = events.try_send(Event::Started(i));
            let start = Instant::now();
            while start.elapsed() < Duration::from_millis(30)
                || self.hold_playing.load(Ordering::SeqCst)
            {
                if let Some(o) = c.outcome() {
                    return o;
                }
                thread::sleep(POLL)
            }
            if i < r.repeat {
                let start = Instant::now();
                while start.elapsed() < GAP {
                    if let Some(o) = c.outcome() {
                        return o;
                    }
                    thread::sleep(POLL)
                }
            }
        }
        Outcome::Completed
    }
}
fn custom_boot() -> (
    Arc<Audio>,
    Arc<CustomBackend>,
    crate::sound_library::Service,
    crate::sound_library::Sound,
    std::path::PathBuf,
) {
    let (library, sound, blob) =
        crate::sound_library::Service::test_registered(crate::builtin_audio::ENTRIES[1].bytes);
    let backend = Arc::new(CustomBackend {
        real: Mp3Backend {
            library: library.clone(),
            ..Default::default()
        },
        hold_playing: AtomicBool::new(false),
        live: AtomicUsize::new(0),
        max: AtomicUsize::new(0),
        trace: Mutex::new(vec![]),
    });
    let audio = Arc::new(Audio::with_backend(
        backend.clone(),
        Duration::from_millis(30),
    ));
    audio.start(|| {}).unwrap();
    until(|| audio.snapshot().state == Phase::Ready);
    (audio, backend, library, sound, blob)
}
fn custom_submit(a: &Audio, id: &str, repeat: u64) -> u64 {
    let end = Instant::now() + Duration::from_secs(1);
    loop {
        match a.submit_custom(false, repeat, 0.37, None, None, id) {
            Ok(request) => return request,
            // Ready can be published while the coordinator briefly owns its
            // mailbox lock. Match the existing builtin test helper's contract.
            Err(e) if e == "AudioMailboxBusy" => {
                assert!(Instant::now() < end); thread::sleep(POLL);
            }
            Err(e) => panic!("{e}"),
        }
    }
}
#[test]
fn custom_normal_repeat_and_half_hour_use_sequential_gap_and_release_pin() {
    let (a, b, l, s, _) = custom_boot();
    let id = custom_submit(&a, &s.id, 2);
    assert_eq!(outcome(&a, id), Outcome::Completed);
    assert_eq!(l.pin_count().unwrap(), 0);
    let trace = b.trace.lock().unwrap();
    assert_eq!(trace.len(), 2);
    assert!(trace[1].duration_since(trace[0]) >= Duration::from_millis(280));
    drop(trace);
    for minute in [0, 30] {
        let repeat = if minute == 0 { 2 } else { 1 };
        let id = a
            .submit_custom(
                true,
                repeat,
                0.37,
                Some(&format!("custom-{minute}")),
                Some(Instant::now() + Duration::from_secs(2)),
                &s.id,
            )
            .unwrap();
        assert_eq!(outcome(&a, id), Outcome::Completed);
        assert_eq!(a.receipt(id).unwrap().repeat, repeat);
    }
    assert_eq!(l.pin_count().unwrap(), 0);
    assert_eq!(b.max.load(Ordering::SeqCst), 1);
    close(&a);
}
#[test]
fn custom_prepare_and_playing_pins_block_unregister_and_failures_release() {
    let (a, b, l, s, _) = custom_boot();
    a.inject(6);
    let id = custom_submit(&a, &s.id, 1);
    until(|| l.pin_count().ok() == Some(1));
    let target = s.id.clone();
    let e = l
        .execute(move |store| store.unregister(&target, 1))
        .unwrap()
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap_err();
    assert_eq!(e.code, "SoundPinned");
    a.cancel();
    assert!(matches!(outcome(&a, id), Outcome::Interrupted(_)));
    assert_eq!(l.pin_count().unwrap(), 0);
    b.hold_playing.store(true, Ordering::SeqCst);
    let id = custom_submit(&a, &s.id, 1);
    until(|| a.snapshot().state == Phase::Playing);
    let target = s.id.clone();
    assert_eq!(
        l.execute(move |store| store.unregister(&target, 1))
            .unwrap()
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap_err()
            .code,
        "SoundPinned"
    );
    a.cancel();
    assert!(matches!(outcome(&a, id), Outcome::Interrupted(_)));
    b.hold_playing.store(false, Ordering::SeqCst);
    for fault in [7, 10] {
        a.inject(fault);
        let id = custom_submit(&a, &s.id, 1);
        assert!(matches!(outcome(&a, id), Outcome::Failed(_)));
        assert_eq!(l.pin_count().unwrap(), 0);
    }
    let id = custom_submit(&a, &s.id, 1);
    assert_eq!(outcome(&a, id), Outcome::Completed);
    close(&a);
}
#[test]
fn custom_stuck_retains_pin_and_single_worker_until_real_acknowledgement() {
    let (a, b, l, s, _) = custom_boot();
    a.inject(8);
    let id = custom_submit(&a, &s.id, 1);
    until(|| l.pin_count().ok() == Some(1));
    let auto = a
        .submit_custom(
            true,
            1,
            0.37,
            Some("custom-stuck-auto"),
            Some(Instant::now() + Duration::from_millis(100)),
            &s.id,
        )
        .unwrap();
    until(|| a.snapshot().state == Phase::Stuck);
    assert!(a.receipt(id).unwrap().outcome.is_none());
    assert_eq!(l.pin_count().unwrap(), 1);
    assert_eq!(b.max.load(Ordering::SeqCst), 1);
    let reject_deadline = Instant::now() + Duration::from_secs(1);
    let rejected = loop {
        let error = a
            .submit_custom(false, 1, 0.37, None, None, &s.id)
            .unwrap_err();
        if error != "AudioMailboxBusy" {
            break error;
        }
        assert!(Instant::now() < reject_deadline, "mailbox retry deadline");
        thread::sleep(POLL);
    };
    assert_eq!(rejected, "AudioStuck");
    assert!(matches!(outcome(&a, auto), Outcome::Skipped(_)));
    assert_eq!(l.pin_count().unwrap(), 1);
    a.inject(9);
    assert!(matches!(outcome(&a, id), Outcome::Interrupted(_)));
    assert_eq!(l.pin_count().unwrap(), 0);
    let recovered = custom_submit(&a, &s.id, 1);
    assert_eq!(outcome(&a, recovered), Outcome::Completed);
    close(&a);
}
#[test]
fn custom_missing_altered_unregistered_and_unregistered_after_delete_are_failed() {
    for missing in [false, true] {
        let (a, _, l, s, blob) = custom_boot();
        if missing {
            std::fs::remove_file(blob).unwrap()
        } else {
            std::fs::write(blob, b"tampered").unwrap()
        }
        let id = custom_submit(&a, &s.id, 1);
        assert!(matches!(outcome(&a, id), Outcome::Failed(_)));
        assert_eq!(l.pin_count().unwrap(), 0);
        close(&a);
    }
    let (a, _, l, s, _) = custom_boot();
    let target = s.id.clone();
    l.execute(move |store| store.unregister(&target, 1))
        .unwrap()
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    for id in [s.id, format!("user:{}", uuid::Uuid::new_v4())] {
        let request = custom_submit(&a, &id, 1);
        assert!(matches!(outcome(&a, request), Outcome::Failed(_)));
        assert_eq!(l.pin_count().unwrap(), 0);
    }
    for path in [
        "C:\\arbitrary.mp3",
        "../arbitrary.mp3",
        "file:///x.mp3",
        "user:bad",
    ] {
        assert_eq!(
            a.submit_custom(false, 1, 0.37, None, None, path)
                .unwrap_err(),
            "InvalidCustomId"
        );
    }
    close(&a);
}
