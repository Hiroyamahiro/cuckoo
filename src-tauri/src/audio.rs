//! One coordinator, at most one playback worker. No join under a shared lock.
#[cfg(test)]
#[path = "audio_tests.rs"]
mod tests;
use rodio::Source;
use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
const GAP: Duration = Duration::from_millis(250);
const POLL: Duration = Duration::from_millis(10);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Kind {
    Probe,
    Test,
    Automatic,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "code")]
pub enum Outcome {
    Completed,
    Interrupted(String),
    Failed(String),
    Skipped(String),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Phase {
    Unknown,
    Preparing,
    Playing,
    Ready,
    Interrupted,
    Failed,
    Unavailable,
    Stuck,
    Stopped,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub id: u64,
    pub kind: Kind,
    pub repeat: u64,
    pub sound_id: String,
    pub phase: String,
    pub outcome: Option<Outcome>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub can_preview: bool,
    pub preview: Option<Preview>,
    pub event_seq: u64,
    pub state: Phase,
    pub available: bool,
    pub can_test: bool,
    pub active: Option<Receipt>,
    pub last_outcome: Option<Receipt>,
    pub message: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub id: u64,
    pub sound_id: Option<String>,
    pub state: String,
}
struct Inner {
    preview_dirty: bool,
    preview_request: Option<Request>,
    preview_click: Option<Instant>,
    snapshot: Snapshot,
    receipts: VecDeque<(Receipt, Instant)>,
    keys: VecDeque<(String, Instant)>,
    queued_test: bool,
    queued_auto: bool,
}
#[derive(Clone)]
struct Request {
    preview: bool,
    id: u64,
    kind: Kind,
    repeat: u64,
    volume: f32,
    deadline: Option<Instant>,
    sound_id: String,
}
impl Request {
    fn receipt(&self) -> Receipt {
        Receipt {
            id: self.id,
            kind: self.kind,
            repeat: self.repeat,
            sound_id: self.sound_id.clone(),
            phase: "Queued".into(),
            outcome: None,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum CancelReason {
    None = 0,
    Preempted = 1,
    ClockCorrection = 2,
    Shutdown = 3,
    Timeout = 4,
    PreviewSwitch = 5,
    PreviewStop = 6,
}
struct Cancel(AtomicU8);
impl Cancel {
    fn request(&self, reason: CancelReason) {
        let _ = self
            .0
            .compare_exchange(0, reason as u8, Ordering::SeqCst, Ordering::SeqCst);
    }
    fn outcome(&self) -> Option<Outcome> {
        match self.0.load(Ordering::SeqCst) {
            0 => None,
            1 => Some(Outcome::Interrupted("PreemptedByChime".into())),
            2 => Some(Outcome::Interrupted("ClockCorrection".into())),
            3 => Some(Outcome::Interrupted("Shutdown".into())),
            5 => Some(Outcome::Interrupted("PreviewSwitched".into())),
            6 => Some(Outcome::Interrupted("PreviewStopped".into())),
            _ => Some(Outcome::Failed("PlaybackTimeout".into())),
        }
    }
}
enum Event {
    Prepared(Duration),
    Started(u64),
    Gap,
}
trait Backend: Send + Sync + 'static {
    #[cfg(any(test, feature = "audio-test-harness"))]
    fn inject(&self, _mode: u8) {}
    fn run(&self, r: &Request, c: &Cancel, events: &SyncSender<Event>) -> Outcome;
}
#[derive(Default)]
struct Mp3Backend {
    library: crate::sound_library::Service,
    #[cfg(any(test, feature = "audio-test-harness"))]
    next_fault: AtomicU8,
    #[cfg(any(test, feature = "audio-test-harness"))]
    release_preparation: AtomicBool,
}
impl Backend for Mp3Backend {
    #[cfg(any(test, feature = "audio-test-harness"))]
    fn inject(&self, mode: u8) {
        if mode == 9 {
            self.release_preparation.store(true, Ordering::SeqCst);
            return;
        }
        if mode == 8 {
            self.release_preparation.store(false, Ordering::SeqCst);
        }
        self.next_fault.store(mode, Ordering::SeqCst);
    }
    fn run(&self, r: &Request, c: &Cancel, events: &SyncSender<Event>) -> Outcome {
        #[cfg(any(test, feature = "audio-test-harness"))]
        let fault = if r.kind != Kind::Probe {
            self.next_fault.swap(0, Ordering::SeqCst)
        } else {
            0
        };
        #[cfg(any(test, feature = "audio-test-harness"))]
        let corrupt = fault == 4;
        #[cfg(not(any(test, feature = "audio-test-harness")))]
        let corrupt = false;
        #[cfg(any(test, feature = "audio-test-harness"))]
        if r.kind != Kind::Probe {
            match fault {
                1 => return Outcome::Failed("DeviceOpenError:Injected".into()),
                2 => return Outcome::Failed("DecodeError:Injected".into()),
                3 => {
                    let _ = events.try_send(Event::Started(1));
                    let started = Instant::now();
                    while started.elapsed() < Duration::from_secs(5) {
                        if let Some(outcome) = c.outcome() {
                            return outcome;
                        }
                        thread::sleep(POLL);
                    }
                    return Outcome::Completed;
                }
                _ => (),
            }
        }
        let result = (|| -> Result<Outcome, String> {
            // Declared first: released only AFTER sink/stream destruction.
            let voice = crate::voicevox::id_valid(&r.sound_id);
            let custom = crate::sound_library::id_valid(&r.sound_id) || voice;
            let mut custom_pin = if custom && !voice {
                Some(
                    self.library
                        .pin_for_playback(&r.sound_id)
                        .map_err(|e| format!("CustomPrepare:{}", e.code))?,
                )
            } else {
                None
            };
            let mut _voice_pin = None;
            let pcm = if voice {
                let (pin,decoded)=crate::voicevox::read_pcm(&r.sound_id)?;
                _voice_pin=Some(pin);
                Some(crate::builtin_audio::TrackedPcm::new(decoded))
            } else if custom {
                #[cfg(any(test, feature = "audio-test-harness"))]
                {
                    if fault == 7 {
                        return Err("DecodeError:InjectedAfterPin".into());
                    }
                    if fault == 6 {
                        let since = Instant::now();
                        while since.elapsed() < Duration::from_secs(5) {
                            if let Some(o) = c.outcome() {
                                return Ok(o);
                            }
                            thread::sleep(POLL);
                        }
                    }
                    if fault == 8 {
                        while !self.release_preparation.load(Ordering::SeqCst) {
                            thread::sleep(POLL);
                        }
                        if let Some(o) = c.outcome() {
                            return Ok(o);
                        }
                    }
                }
                let decoded = match custom_pin
                    .as_mut()
                    .unwrap()
                    .decode(|| c.outcome().is_some())
                {
                    Ok(pcm) => pcm,
                    Err(e) => {
                        return Ok(c.outcome().unwrap_or_else(|| {
                            Outcome::Failed(format!("CustomPrepare:{}", e.code))
                        }))
                    }
                };
                eprintln!(
                    "CUSTOM_PREPARED id={} source={} frames={} pin=held",
                    r.id,
                    r.sound_id,
                    decoded.samples.len() / decoded.channels as usize
                );
                Some(crate::builtin_audio::TrackedPcm::new(decoded))
            } else {
                let (entry, _) =
                    crate::builtin_audio::plan(&r.sound_id, r.repeat).ok_or("UnknownBuiltin")?;
                let bytes = if corrupt {
                    &entry.bytes[..entry.bytes.len() - 1]
                } else {
                    entry.bytes
                };
                let decoded = match crate::mp3_decode::decode(bytes, || c.outcome().is_some()) {
                    Ok(pcm) => pcm,
                    Err(e) => {
                        if let Some(outcome) = c.outcome() {
                            return Ok(outcome);
                        }
                        return Err(format!("DecodeError:{e}"));
                    }
                };
                eprintln!("MP3_DECODE id={} source={} packets={} frames={} rate={} channels={} samples={} pcmCapacityBytes={} workBufferBytes={}",r.id,entry.id,decoded.packets,decoded.mpeg_frames,decoded.rate,decoded.channels,decoded.samples.len(),decoded.samples.capacity()*4,decoded.work_buffer_bytes);
                Some(crate::builtin_audio::TrackedPcm::new(decoded))
            };
            #[cfg(any(test, feature = "audio-test-harness"))]
            if custom && fault == 10 {
                return Err("DeviceOpenError:InjectedAfterPin".into());
            }
            let length = if let Some(pcm) = &pcm {
                pcm.pcm.duration()
            } else {
                return Err("DecodeError:MissingPcm".into());
            };
            let _ = events.try_send(Event::Prepared(length));
            if let Some(o) = c.outcome() {
                return Ok(o);
            }
            let (_stream, handle) =
                rodio::OutputStream::try_default().map_err(|e| format!("DeviceOpenError:{e}"))?;
            if let Some(o) = c.outcome() {
                return Ok(o);
            }
            if r.kind == Kind::Probe {
                return Ok(Outcome::Completed);
            }
            if let Some(pcm) = pcm.as_ref().filter(|_| !custom) {
                let (entry, delay) =
                    crate::builtin_audio::plan(&r.sound_id, r.repeat).ok_or("UnknownBuiltin")?;
                let tx = events.clone();
                let id = r.id;
                let source = crate::builtin_audio::TimedPcm::new(pcm.clone(), delay, move || {
                    let _ = tx.try_send(Event::Started(2));
                    eprintln!("AUDIO_VOICE_RENDER id={id} iteration=2 source={} offsetMs={} (sample render, not hardware onset)", entry.id, delay.unwrap().as_millis());
                })?;
                let body = source.total_duration().ok_or("MissingDuration")?;
                let sink =
                    rodio::Sink::try_new(&handle).map_err(|e| format!("DeviceOpenError:{e}"))?;
                sink.set_volume(r.volume);
                sink.append(source);
                let _ = events.try_send(Event::Started(1));
                eprintln!("AUDIO_PLAN id={} source={} logicalRepeat={} voices={} secondOffsetMs={:?} bodyMs={}",r.id,entry.id,r.repeat,if delay.is_some(){2}else{1},delay.map(|d|d.as_millis()),body.as_millis());
                let start = Instant::now();
                while !sink.empty() {
                    if let Some(o) = c.outcome() {
                        sink.stop();
                        return Ok(o);
                    }
                    #[cfg(any(test, feature = "audio-test-harness"))]
                    if fault == 5
                        && start.elapsed() > delay.unwrap_or_default() + Duration::from_millis(150)
                    {
                        sink.stop();
                        return Err("DevicePlaybackError:InjectedDuringMix".into());
                    }
                    if start.elapsed() > body + Duration::from_secs(3).max(body / 10) {
                        sink.stop();
                        return Err("PlaybackTimeout".into());
                    }
                    thread::sleep(POLL);
                }
                return Ok(c.outcome().unwrap_or(Outcome::Completed));
            }
            for iteration in 1..=r.repeat {
                if let Some(o) = c.outcome() {
                    return Ok(o);
                }
                let sink =
                    rodio::Sink::try_new(&handle).map_err(|e| format!("DeviceOpenError:{e}"))?;
                sink.set_volume(r.volume);
                if let Some(pcm) = &pcm {
                    sink.append(crate::builtin_audio::SharedPcm::new(pcm.clone()));
                } else {
                    return Err("DecodeError:MissingPcm".into());
                }
                let _ = events.try_send(Event::Started(iteration));
                eprintln!(
                    "AUDIO_START {} id={} iteration={iteration} repeat={} source={}",
                    chrono::Local::now().to_rfc3339(),
                    r.id,
                    r.repeat,
                    r.sound_id
                );
                let start = Instant::now();
                while !sink.empty() {
                    #[cfg(any(test, feature = "audio-test-harness"))]
                    if custom && fault == 11 && start.elapsed() > Duration::from_millis(150) {
                        sink.stop();
                        return Err("DevicePlaybackError:InjectedCustom".into());
                    }
                    if let Some(o) = c.outcome() {
                        sink.stop();
                        return Ok(o);
                    }
                    if start.elapsed() > length + Duration::from_millis(3000).max(length / 10) {
                        sink.stop();
                        return Err("PlaybackTimeout".into());
                    }
                    thread::sleep(POLL);
                }
                if let Some(o) = c.outcome() {
                    return Ok(o);
                }
                if iteration < r.repeat {
                    let _ = events.try_send(Event::Gap);
                    let start = Instant::now();
                    while start.elapsed() < GAP {
                        if let Some(o) = c.outcome() {
                            return Ok(o);
                        }
                        thread::sleep(POLL)
                    }
                }
            }
            Ok(Outcome::Completed)
        })();
        result.unwrap_or_else(Outcome::Failed)
    }
}
struct Active {
    request: Request,
    cancel: Arc<Cancel>,
    handle: thread::JoinHandle<Outcome>,
    events: Receiver<Event>,
    started: bool,
    start: Instant,
    limit: Duration,
    cancelling: Option<Instant>,
    stuck: bool,
}
pub struct Audio {
    inner: Mutex<Inner>,
    auto_tx: SyncSender<Request>,
    test_tx: SyncSender<Request>,
    receivers: Mutex<Option<(Receiver<Request>, Receiver<Request>)>>,
    next: AtomicU64,
    stop: AtomicBool,
    running: AtomicBool,
    correction: AtomicU64,
    coordinator: Mutex<Option<thread::JoinHandle<()>>>,
    backend: Arc<dyn Backend>,
    stop_ack: Duration,
}
impl Default for Audio {
    fn default() -> Self {
        Self::with_backend(Arc::new(Mp3Backend::default()), Duration::from_millis(500))
    }
}
impl Audio {
    pub fn with_library(library: crate::sound_library::Service) -> Self {
        Self::with_backend(
            Arc::new(Mp3Backend {
                library,
                ..Default::default()
            }),
            Duration::from_millis(500),
        )
    }
    pub fn submit_custom(
        &self,
        automatic: bool,
        repeat: u64,
        volume: f32,
        key: Option<&str>,
        deadline: Option<Instant>,
        sound_id: &str,
    ) -> Result<u64, String> {
        if !crate::sound_library::id_valid(sound_id) && !crate::voicevox::id_valid(sound_id) {
            return Err("InvalidCustomId".into());
        }
        self.submit_source(automatic, repeat, volume, key, deadline, sound_id.into())
    }
    #[cfg(any(test, feature = "audio-test-harness"))]
    pub fn inject(&self, mode: u8) {
        self.backend.inject(mode);
    }
    fn with_backend(backend: Arc<dyn Backend>, stop_ack: Duration) -> Self {
        let (auto_tx, a) = mpsc::sync_channel(1);
        let (test_tx, t) = mpsc::sync_channel(1);
        Self {
            inner: Mutex::new(Inner {
                snapshot: Snapshot {
                    can_preview: false,
                    preview: None,
                    event_seq: 0,
                    state: Phase::Unknown,
                    available: false,
                    can_test: false,
                    active: None,
                    last_outcome: None,
                    message: "音声の準備を確認しています".into(),
                },
                receipts: VecDeque::new(),
                preview_dirty: false,
                preview_request: None,
                preview_click: None,
                keys: VecDeque::new(),
                queued_test: false,
                queued_auto: false,
            }),
            auto_tx,
            test_tx,
            receivers: Mutex::new(Some((a, t))),
            next: AtomicU64::new(1),
            stop: AtomicBool::new(false),
            running: AtomicBool::new(false),
            correction: AtomicU64::new(0),
            coordinator: Mutex::new(None),
            backend,
            stop_ack,
        }
    }
    pub fn snapshot(&self) -> Snapshot {
        self.inner.lock().unwrap().snapshot.clone()
    }
    pub fn receipt(&self, id: u64) -> Option<Receipt> {
        self.inner
            .lock()
            .unwrap()
            .receipts
            .iter()
            .find(|(r, _)| r.id == id)
            .map(|(r, _)| r.clone())
    }
    fn mutate(&self, f: impl FnOnce(&mut Inner)) {
        let mut i = self.inner.lock().unwrap();
        f(&mut i);
        i.snapshot.event_seq += 1;
        i.snapshot.can_preview = self.running.load(Ordering::SeqCst)
            && !self.stop.load(Ordering::SeqCst)
            && !i.queued_auto
            && !matches!(
                i.snapshot.state,
                Phase::Stuck | Phase::Stopped | Phase::Unknown
            )
            && i.snapshot
                .active
                .as_ref()
                .is_none_or(|a| a.kind == Kind::Test);
        i.snapshot.can_test = self.running.load(Ordering::SeqCst)
            && !self.stop.load(Ordering::SeqCst)
            && i.snapshot.active.is_none()
            && !i.queued_test
            && !i.queued_auto
            && i.snapshot.state != Phase::Stuck;
    }
    fn update(&self, r: Receipt) {
        self.mutate(|i| {
            if let Some((old, _)) = i.receipts.iter_mut().find(|(old, _)| old.id == r.id) {
                *old = r.clone()
            }
            i.snapshot.active = Some(r)
        });
    }
    fn finish(&self, r: &Request, outcome: Outcome) {
        eprintln!(
            "AUDIO_RESULT {} id={} kind={:?} repeat={} {:?}",
            chrono::Local::now().to_rfc3339(),
            r.id,
            r.kind,
            r.repeat,
            outcome
        );
        let mut receipt = r.receipt();
        receipt.phase = "Finished".into();
        receipt.outcome = Some(outcome.clone());
        self.mutate(|i| {
            // A superseded intent must not keep claiming Stuck after the real
            // retained worker has finally acknowledged its termination.
            if i.snapshot.active.as_ref().is_some_and(|a| a.id == r.id) {
                if let Some(p) = i
                    .snapshot
                    .preview
                    .as_mut()
                    .filter(|p| p.id != r.id && p.state == "Stuck")
                {
                    p.state = "Skipped".into();
                }
            }
            if let Some(p) = i.snapshot.preview.as_mut().filter(|p| p.id == r.id) {
                p.state = match &outcome {
                    Outcome::Completed => "Completed",
                    Outcome::Interrupted(_) => "Interrupted",
                    Outcome::Failed(_) => "Failed",
                    Outcome::Skipped(_) => "Skipped",
                }
                .into();
            }
            if let Some((old, t)) = i.receipts.iter_mut().find(|(old, _)| old.id == r.id) {
                *old = receipt.clone();
                *t = Instant::now()
            }
            let current = i.snapshot.active.as_ref().is_none_or(|a| a.id == r.id);
            if current {
                i.snapshot.active = None;
                match &outcome {
                    Outcome::Completed => {
                        i.snapshot.state = Phase::Ready;
                        i.snapshot.available = true;
                        i.snapshot.message = if r.kind == Kind::Probe {
                            "音声の準備確認済み・時報待機中"
                        } else {
                            "再生完了・時報待機中"
                        }
                        .into();
                    }
                    Outcome::Interrupted(code) => {
                        i.snapshot.state = Phase::Interrupted;
                        i.snapshot.message = format!("再生を中断しました：{code}");
                    }
                    Outcome::Failed(code) => {
                        i.snapshot.available = false;
                        i.snapshot.state = if code.starts_with("DeviceOpenError") {
                            Phase::Unavailable
                        } else {
                            Phase::Failed
                        };
                        i.snapshot.message = format!("音声エラー：{code}");
                    }
                    Outcome::Skipped(code) => {
                        i.snapshot.state = if i.snapshot.available {
                            Phase::Ready
                        } else {
                            Phase::Unavailable
                        };
                        i.snapshot.message = format!("時報を再生しませんでした：{code}");
                    }
                }
            }
            i.snapshot.last_outcome = Some(receipt);
        });
    }
    /// The scheduler never waits for a coordinator/worker lock.
    #[cfg(any(test, feature = "audio-test-harness"))]
    pub fn submit(
        &self,
        automatic: bool,
        repeat: u64,
        volume: f32,
        key: Option<&str>,
        deadline: Option<Instant>,
    ) -> Result<u64, String> {
        self.submit_source(
            automatic,
            repeat,
            volume,
            key,
            deadline,
            crate::builtin_audio::DEFAULT_ID.into(),
        )
    }
    pub fn submit_builtin(
        &self,
        automatic: bool,
        repeat: u64,
        volume: f32,
        key: Option<&str>,
        deadline: Option<Instant>,
        sound_id: &str,
    ) -> Result<u64, String> {
        let entry = crate::builtin_audio::find(sound_id).ok_or("UnknownBuiltin")?;
        self.submit_source(automatic, repeat, volume, key, deadline, entry.id.into())
    }
    fn submit_source(
        &self,
        automatic: bool,
        repeat: u64,
        volume: f32,
        key: Option<&str>,
        deadline: Option<Instant>,
        sound_id: String,
    ) -> Result<u64, String> {
        if ![1, 2].contains(&repeat) || !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
            return Err("InvalidPlayback".into());
        }
        let repeat=crate::voicevox::playback_repeat(&sound_id,repeat);
        if self.stop.load(Ordering::SeqCst) {
            return Err("AudioStopping".into());
        }
        let mut i = self.inner.try_lock().map_err(|_| "AudioMailboxBusy")?;
        if self.stop.load(Ordering::SeqCst) || !self.running.load(Ordering::SeqCst) {
            return Err("AudioUnavailable".into());
        }
        if i.snapshot.state == Phase::Stuck {
            return Err("AudioStuck".into());
        }
        if automatic {
            if key.is_none() {
                return Err("MissingScheduledKey".into());
            }
            if i.keys.iter().any(|(k, _)| Some(k.as_str()) == key) {
                return Err("DuplicateChime".into());
            }
            if i.queued_auto {
                return Err("AudioMailboxBusy".into());
            }
        } else if i.queued_test || i.queued_auto || i.snapshot.active.is_some() {
            return Err("AudioBusy".into());
        }
        if i.receipts.len() >= 256 {
            if let Some(p) = i.receipts.iter().position(|(r, _)| r.outcome.is_some()) {
                i.receipts.remove(p);
            } else {
                return Err("AudioReceiptFull".into());
            }
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let r = Request {
            preview: false,
            id,
            kind: if automatic {
                Kind::Automatic
            } else {
                Kind::Test
            },
            repeat,
            volume,
            deadline,
            sound_id,
        };
        let tx = if automatic {
            &self.auto_tx
        } else {
            &self.test_tx
        };
        tx.try_send(r.clone()).map_err(|_| "AudioMailboxBusy")?;
        i.receipts.push_back((r.receipt(), Instant::now()));
        if automatic {
            i.queued_auto = true;
            i.keys.push_back((key.unwrap().into(), Instant::now()));
            while i.keys.len() > 384 {
                i.keys.pop_front();
            }
        } else {
            i.queued_test = true;
        }
        i.snapshot.can_test = false;
        if automatic {
            i.snapshot.can_preview = false;
        }
        i.snapshot.event_seq += 1;
        Ok(id)
    }
    /// Latest-wins preview mailbox. No file IO, decode, cancellation wait or join here.
    pub fn toggle_preview(&self, sound_id: &str, repeat: u64, volume: f32) -> Result<u64, String> {
        if ![1, 2].contains(&repeat) || !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
            return Err("InvalidPlayback".into());
        }
        let repeat=crate::voicevox::playback_repeat(sound_id,repeat);
        if crate::builtin_audio::find(sound_id).is_none()
            && !crate::sound_library::id_valid(sound_id)
            && !crate::voicevox::id_valid(sound_id)
        {
            return Err("UnknownSoundId".into());
        }
        let mut i = self.inner.lock().map_err(|_| "AudioUnavailable")?;
        if self.stop.load(Ordering::SeqCst) || !self.running.load(Ordering::SeqCst) {
            return Err("AudioUnavailable".into());
        }
        if i.snapshot.state == Phase::Stuck {
            return Err("AudioStuck".into());
        }
        if i.queued_auto
            || i.snapshot
                .active
                .as_ref()
                .is_some_and(|a| a.kind != Kind::Test)
        {
            return Err("AutomaticPriority".into());
        }
        if i.preview_click
            .is_some_and(|t| t.elapsed() < Duration::from_millis(300))
        {
            return Err("PreviewCooldown".into());
        }
        let same = i.snapshot.preview.as_ref().is_some_and(|p| {
            p.sound_id.as_deref() == Some(sound_id)
                && matches!(p.state.as_str(), "Queued" | "Switching" | "Playing")
        }) || (i
            .snapshot
            .active
            .as_ref()
            .is_some_and(|a| a.sound_id == sound_id));
        if i.receipts.len() >= 256 {
            if let Some(n) = i.receipts.iter().position(|(r, _)| r.outcome.is_some()) {
                i.receipts.remove(n);
            } else {
                return Err("AudioReceiptFull".into());
            }
        }
        if let Some(old) = i.preview_request.take() {
            if let Some((r, _)) = i.receipts.iter_mut().find(|(r, _)| r.id == old.id) {
                r.phase = "Finished".into();
                r.outcome = Some(Outcome::Skipped("Superseded".into()));
            }
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        i.preview_click = Some(Instant::now());
        let state = if same {
            "Stopping"
        } else if i.snapshot.active.is_some() {
            "Switching"
        } else {
            "Queued"
        };
        i.snapshot.preview = Some(Preview {
            id,
            sound_id: (!same).then(|| sound_id.to_string()),
            state: state.into(),
        });
        if !same {
            let r = Request {
                preview: true,
                id,
                kind: Kind::Test,
                repeat,
                volume,
                deadline: None,
                sound_id: sound_id.into(),
            };
            i.receipts.push_back((r.receipt(), Instant::now()));
            i.preview_request = Some(r);
        }
        i.preview_dirty = true;
        i.snapshot.event_seq += 1;
        Ok(id)
    }
    pub fn cancel(&self) {
        self.correction.fetch_add(1, Ordering::SeqCst);
    }
    pub fn start(self: &Arc<Self>, notify: impl Fn() + Send + 'static) -> Result<(), String> {
        let mut slot = self.coordinator.lock().unwrap();
        if slot.is_some() {
            return Err("AudioAlreadyStarted".into());
        }
        let receivers = self
            .receivers
            .lock()
            .unwrap()
            .take()
            .ok_or("AudioAlreadyStarted")?;
        let audio = self.clone();
        self.running.store(true, Ordering::SeqCst);
        let spawned = thread::Builder::new()
            .name("cuckoo-audio-coordinator".into())
            .spawn(move || audio.run(receivers, notify));
        match spawned {
            Ok(handle) => *slot = Some(handle),
            Err(e) => {
                self.running.store(false, Ordering::SeqCst);
                self.mutate(|i| {
                    i.snapshot.state = Phase::Unavailable;
                    i.snapshot.message = format!("音声エラー：coordinatorを開始できません：{e}");
                });
                return Err(e.to_string());
            }
        }
        Ok(())
    }
    fn launch(&self, r: Request) -> Result<Active, (Request, Outcome)> {
        // Reservation and phase change are atomic with respect to submit().
        let mut receipt = r.receipt();
        receipt.phase = "Preparing".into();
        let mut superseded = false;
        self.mutate(|i| {
            if r.preview
                && (i.queued_auto || i.snapshot.preview.as_ref().is_none_or(|p| p.id != r.id))
            {
                superseded = true;
                return;
            }
            match r.kind {
                Kind::Automatic => i.queued_auto = false,
                Kind::Test => i.queued_test = false,
                Kind::Probe => (),
            };
            i.snapshot.active = Some(receipt.clone());
            if let Some((old, _)) = i.receipts.iter_mut().find(|(old, _)| old.id == r.id) {
                *old = receipt;
            }
            i.snapshot.state = Phase::Preparing;
            i.snapshot.message = "音声を準備しています".into();
        });
        if superseded {
            return Err((r, Outcome::Skipped("Superseded".into())));
        }
        let cancel = Arc::new(Cancel(AtomicU8::new(0)));
        let worker_cancel = cancel.clone();
        let backend = self.backend.clone();
        let request = r.clone();
        let (events_tx, events) = mpsc::sync_channel(16);
        let handle = thread::Builder::new()
            .name("cuckoo-audio-playback".into())
            .spawn(move || {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    backend.run(&request, &worker_cancel, &events_tx)
                }))
                .unwrap_or_else(|_| Outcome::Failed("WorkerPanic".into()))
            });
        match handle {
            Ok(handle) => Ok(Active {
                request: r,
                cancel,
                handle,
                events,
                started: false,
                start: Instant::now(),
                limit: Duration::from_secs(15),
                cancelling: None,
                stuck: false,
            }),
            Err(e) => Err((r, Outcome::Failed(format!("WorkerSpawn:{e}")))),
        }
    }
    fn discard_pending(&self, r: Request, reason: &str) {
        self.mutate(|i| match r.kind {
            Kind::Automatic => i.queued_auto = false,
            Kind::Test => i.queued_test = false,
            Kind::Probe => (),
        });
        self.finish(
            &r,
            if r.preview {
                Outcome::Skipped(reason.into())
            } else {
                Outcome::Interrupted(reason.into())
            },
        );
    }
    fn run(&self, (auto_rx, test_rx): (Receiver<Request>, Receiver<Request>), notify: impl Fn()) {
        let mut active = match self.launch(Request {
            preview: false,
            id: 0,
            kind: Kind::Probe,
            repeat: 1,
            volume: 0.0,
            deadline: None,
            sound_id: crate::builtin_audio::DEFAULT_ID.into(),
        }) {
            Ok(a) => Some(a),
            Err((r, o)) => {
                self.finish(&r, o);
                None
            }
        };
        let mut pending_auto: Option<Request> = None;
        let mut pending_test: Option<Request> = None;
        let mut correction = self.correction.load(Ordering::SeqCst);
        let mut published = u64::MAX;
        loop {
            let stop = self.stop.load(Ordering::SeqCst);
            let current_correction = self.correction.load(Ordering::SeqCst);
            let changed_clock = correction != current_correction;
            correction = current_correction;
            if pending_auto.is_none() {
                pending_auto = auto_rx.try_recv().ok();
            }
            if pending_test.is_none() {
                pending_test = test_rx.try_recv().ok();
            }
            let (preview_changed, desired) = {
                let mut i = self.inner.lock().unwrap();
                let dirty = std::mem::take(&mut i.preview_dirty);
                (
                    dirty,
                    if dirty {
                        i.preview_request.take()
                    } else {
                        None
                    },
                )
            };
            if preview_changed {
                if let Some(old) = pending_test.take() {
                    self.mutate(|i| i.queued_test = false);
                    self.finish(&old, Outcome::Skipped("Superseded".into()));
                }
                pending_test = desired;
            }
            if stop || changed_clock {
                for r in [pending_auto.take(), pending_test.take()]
                    .into_iter()
                    .flatten()
                {
                    self.discard_pending(r, if stop { "Shutdown" } else { "ClockCorrection" });
                }
            }
            if pending_auto.is_some() {
                if let Some(r) = pending_test.take() {
                    self.discard_pending(r, "PreemptedByChime");
                }
            }
            if let Some(a) = active.as_mut() {
                let reason = if stop {
                    CancelReason::Shutdown
                } else if changed_clock {
                    CancelReason::ClockCorrection
                } else if pending_auto.is_some() {
                    CancelReason::Preempted
                } else if a.request.kind == Kind::Test
                    && (pending_test.as_ref().is_some_and(|r| r.preview)
                        || (preview_changed && pending_test.is_none()))
                {
                    if pending_test.is_some() {
                        CancelReason::PreviewSwitch
                    } else {
                        CancelReason::PreviewStop
                    }
                } else {
                    CancelReason::None
                };
                if reason != CancelReason::None && !a.handle.is_finished() {
                    a.cancel.request(reason);
                }
                while let Ok(event) = a.events.try_recv() {
                    match event {
                        Event::Prepared(length) => {
                            let body = if crate::sound_library::id_valid(&a.request.sound_id)
                            {
                                length * a.request.repeat as u32
                                    + GAP * (a.request.repeat.saturating_sub(1) as u32)
                            } else {
                                length
                                    + crate::builtin_audio::plan(
                                        &a.request.sound_id,
                                        a.request.repeat,
                                    )
                                    .and_then(|(_, delay)| delay)
                                    .unwrap_or_default()
                            };
                            a.limit = Duration::from_secs(5)
                                + body
                                + Duration::from_secs(5).max(body / 10);
                        }
                        Event::Started(iteration) => {
                            a.started = true;
                            if !a.stuck {
                                let mut r = a.request.receipt();
                                r.phase = format!("Playing:{iteration}");
                                self.update(r);
                                self.mutate(|i| {
                                    if let Some(p) =
                                        i.snapshot.preview.as_mut().filter(|p| p.id == a.request.id)
                                    {
                                        p.state = "Playing".into();
                                    }
                                    i.snapshot.available = true;
                                    i.snapshot.state = Phase::Playing;
                                    i.snapshot.message = "音声を再生中です".into()
                                });
                            }
                        }
                        Event::Gap => {
                            if !a.stuck {
                                let mut r = a.request.receipt();
                                r.phase = "RepeatGap".into();
                                self.update(r);
                            }
                        }
                    }
                }
                if (!a.started
                    && (a.start.elapsed() > Duration::from_secs(5)
                        || a.request.deadline.is_some_and(|d| Instant::now() > d)))
                    || a.start.elapsed() > a.limit
                {
                    a.cancel.request(CancelReason::Timeout);
                }
                if a.cancel.outcome().is_some() {
                    let since = *a.cancelling.get_or_insert_with(Instant::now);
                    if since.elapsed() > self.stop_ack && !a.handle.is_finished() && !a.stuck {
                        a.stuck = true;
                        let mut r = a.request.receipt();
                        r.phase = "Stuck".into();
                        self.update(r);
                        self.mutate(|i|{i.snapshot.state=Phase::Stuck;i.snapshot.available=false;i.snapshot.message="音声エラー：停止を確認できません。実行中の音声処理を保持しています".into();});
                        if let Some(r) = pending_test.take() {
                            self.finish(&r, Outcome::Skipped("AudioStuck".into()));
                        }
                        self.mutate(|i| {
                            if let Some(p) = i.snapshot.preview.as_mut() {
                                p.state = "Stuck".into();
                            }
                        });
                    }
                }
            }
            if active.as_ref().is_some_and(|a| a.stuck) {
                if let Some(r) = pending_test.take() {
                    self.finish(&r, Outcome::Skipped("AudioStuck".into()));
                }
                let needs = self
                    .snapshot()
                    .preview
                    .as_ref()
                    .is_some_and(|p| p.state != "Stuck");
                if needs {
                    self.mutate(|i| {
                        if let Some(p) = i.snapshot.preview.as_mut() {
                            p.state = "Stuck".into();
                        }
                    });
                }
            }
            if active.as_ref().is_some_and(|a| a.handle.is_finished()) {
                let a = active.take().unwrap();
                let mut outcome = a
                    .handle
                    .join()
                    .unwrap_or_else(|_| Outcome::Failed("WorkerPanic".into()));
                // A cancellation request is not proof that playback was interrupted.
                // Trust the worker's terminal acknowledgement; only the service's
                // own elapsed deadline turns a late natural completion into failure.
                if outcome == Outcome::Completed
                    && a.cancel.0.load(Ordering::SeqCst) == CancelReason::Timeout as u8
                {
                    outcome = Outcome::Failed("PlaybackTimeout".into());
                }
                self.finish(&a.request, outcome);
            }
            if pending_auto
                .as_ref()
                .is_some_and(|r| r.deadline.is_some_and(|d| Instant::now() > d))
            {
                let r = pending_auto.take().unwrap();
                self.mutate(|i| i.queued_auto = false);
                self.finish(&r, Outcome::Skipped("DeadlineExceeded".into()));
            }
            if active.is_none() && !stop {
                if let Some(r) = pending_auto.take().or_else(|| pending_test.take()) {
                    if r.deadline.is_some_and(|d| Instant::now() > d) {
                        self.mutate(|i| match r.kind {
                            Kind::Automatic => i.queued_auto = false,
                            Kind::Test => i.queued_test = false,
                            Kind::Probe => (),
                        });
                        self.finish(&r, Outcome::Skipped("DeadlineExceeded".into()));
                    } else {
                        match self.launch(r) {
                            Ok(a) => active = Some(a),
                            Err((r, o)) => self.finish(&r, o),
                        }
                    }
                }
            }
            if active.is_none() && pending_test.is_none() {
                let needs = self
                    .snapshot()
                    .preview
                    .as_ref()
                    .is_some_and(|p| p.state == "Stopping");
                if needs {
                    self.mutate(|i| {
                        if let Some(p) = i.snapshot.preview.as_mut() {
                            p.state = "Stopped".into();
                        }
                    });
                }
            }
            if stop && active.is_none() {
                self.mutate(|i| {
                    i.snapshot.state = Phase::Stopped;
                    i.snapshot.available = false;
                    i.snapshot.message = "音声を停止しました".into();
                });
                notify();
                break;
            }
            let seq = self.snapshot().event_seq;
            if seq != published {
                published = seq;
                notify();
            }
            {
                let mut i = self.inner.lock().unwrap();
                i.receipts
                    .retain(|(r, t)| r.outcome.is_none() || t.elapsed() < Duration::from_secs(600));
                i.keys
                    .retain(|(_, t)| t.elapsed() < Duration::from_secs(7 * 86400));
            }
            thread::sleep(POLL);
        }
    }
    /// false means a real worker may still be inside an OS API. No forced detach.
    pub fn shutdown(&self, timeout: Duration) -> bool {
        self.mutate(|_| self.stop.store(true, Ordering::SeqCst));
        let end = Instant::now() + timeout;
        while Instant::now() < end {
            let finished = self
                .coordinator
                .lock()
                .unwrap()
                .as_ref()
                .is_none_or(|h| h.is_finished());
            if finished {
                let handle = self.coordinator.lock().unwrap().take();
                if let Some(h) = handle {
                    let _ = h.join();
                }
                return true;
            }
            thread::sleep(POLL);
        }
        false
    }
}
