//! Immutable builtins. A PCM allocation belongs only to a single live request.
use crate::mp3_decode::Pcm;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
pub const DEFAULT_ID: &str = "builtin:cuckoo-clock";
pub struct Entry {
    pub id: &'static str,
    pub bytes: &'static [u8],
}
pub static ENTRIES: [Entry; 4] = [
    Entry {
        id: "builtin:cuckoo-clock",
        bytes: include_bytes!("../../assets/sounds/builtin_mp3/cuckoo-clock.mp3"),
    },
    Entry {
        id: "builtin:church-bell",
        bytes: include_bytes!("../../assets/sounds/builtin_mp3/church-bell.mp3"),
    },
    Entry {
        id: "builtin:pendulum-clock",
        bytes: include_bytes!("../../assets/sounds/builtin_mp3/pendulum-clock.mp3"),
    },
    Entry {
        id: "builtin:school-bell",
        bytes: include_bytes!("../../assets/sounds/builtin_mp3/school-bell.mp3"),
    },
];
pub fn find(id: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|e| e.id == id)
}
pub const SCHOOL_LONG: Entry = Entry {
    id: "builtin:school-bell-midium",
    bytes: include_bytes!("../../assets/sounds/builtin_mp3/school-bell_midium.mp3"),
};
/// Resolve logical repeat to physical voices, without changing saved v7 settings.
pub fn plan(id: &str, repeat: u64) -> Option<(&'static Entry, Option<Duration>)> {
    let entry = find(id)?;
    if repeat == 1 {
        return Some((entry, None));
    }
    if repeat != 2 {
        return None;
    }
    Some(match id {
        "builtin:cuckoo-clock" => (entry, Some(Duration::from_millis(1367))),
        "builtin:church-bell" | "builtin:pendulum-clock" => (entry, Some(Duration::from_secs(2))),
        "builtin:school-bell" => (&SCHOOL_LONG, None),
        _ => return None,
    })
}

/// Two sample-timed voices, one sink, one shared immutable PCM allocation.
pub struct TimedPcm {
    data: Arc<TrackedPcm>,
    position: usize,
    second: Option<usize>,
    on_second: Option<Box<dyn FnOnce() + Send>>,
}
impl TimedPcm {
    pub fn new(
        data: Arc<TrackedPcm>,
        delay: Option<Duration>,
        on_second: impl FnOnce() + Send + 'static,
    ) -> Result<Self, String> {
        let second = delay.map(|d| {
            ((d.as_nanos() * data.pcm.rate as u128 / 1_000_000_000) as usize)
                * data.pcm.channels as usize
        });
        if let Some(offset) = second {
            // Fixed sources must be safe even at volume 100%; no hidden limiter/normalization.
            for i in offset..data.pcm.samples.len() {
                if (data.pcm.samples[i] + data.pcm.samples[i - offset]).abs() >= 1.0 {
                    return Err("MixClipping".into());
                }
            }
        }
        Ok(Self {
            data,
            position: 0,
            second,
            on_second: Some(Box::new(on_second)),
        })
    }
    fn length(&self) -> usize {
        self.data.pcm.samples.len() + self.second.unwrap_or(0)
    }
}
impl Iterator for TimedPcm {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.position >= self.length() {
            return None;
        }
        if self.second == Some(self.position) {
            if let Some(notify) = self.on_second.take() {
                notify();
            }
        }
        let first = self
            .data
            .pcm
            .samples
            .get(self.position)
            .copied()
            .unwrap_or(0.);
        let second = self
            .second
            .and_then(|offset| self.position.checked_sub(offset))
            .and_then(|i| self.data.pcm.samples.get(i))
            .copied()
            .unwrap_or(0.);
        self.position += 1;
        Some(first + second)
    }
}
impl rodio::Source for TimedPcm {
    fn current_frame_len(&self) -> Option<usize> {
        Some(self.length() - self.position)
    }
    fn channels(&self) -> u16 {
        self.data.pcm.channels
    }
    fn sample_rate(&self) -> u32 {
        self.data.pcm.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f64(
            self.length() as f64 / self.channels() as f64 / self.sample_rate() as f64,
        ))
    }
}
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
pub fn pcm_stats() -> serde_json::Value {
    serde_json::json!({"liveBytes":LIVE.load(Ordering::SeqCst),"peakBytes":PEAK.load(Ordering::SeqCst),"scope":"PCM Vec capacity only; excludes decoder/driver/allocator overhead"})
}
pub struct TrackedPcm {
    pub pcm: Pcm,
    allocated: usize,
}
impl TrackedPcm {
    pub fn new(pcm: Pcm) -> Arc<Self> {
        let allocated = pcm.samples.capacity() * std::mem::size_of::<f32>();
        let live = LIVE.fetch_add(allocated, Ordering::SeqCst) + allocated;
        PEAK.fetch_max(live, Ordering::SeqCst);
        Arc::new(Self { pcm, allocated })
    }
}
impl Drop for TrackedPcm {
    fn drop(&mut self) {
        LIVE.fetch_sub(self.allocated, Ordering::SeqCst);
    }
}
pub struct SharedPcm {
    data: Arc<TrackedPcm>,
    offset: usize,
}
impl SharedPcm {
    pub fn new(data: Arc<TrackedPcm>) -> Self {
        Self { data, offset: 0 }
    }
}
impl Iterator for SharedPcm {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let value = self.data.pcm.samples.get(self.offset).copied();
        if value.is_some() {
            self.offset += 1;
        }
        value
    }
}
impl rodio::Source for SharedPcm {
    fn current_frame_len(&self) -> Option<usize> {
        Some(self.data.pcm.samples.len() - self.offset)
    }
    fn channels(&self) -> u16 {
        self.data.pcm.channels
    }
    fn sample_rate(&self) -> u32 {
        self.data.pcm.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        Some(self.data.pcm.duration())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    #[test]
    fn approved_plans_mix_at_exact_sample_offsets_without_clipping() {
        for entry in &ENTRIES {
            for repeat in [1, 2] {
                let (resolved, delay) = plan(entry.id, repeat).unwrap();
                if entry.id == "builtin:school-bell" && repeat == 2 {
                    assert_eq!(resolved.id, SCHOOL_LONG.id);
                    assert!(delay.is_none());
                } else {
                    assert_eq!(resolved.id, entry.id);
                }
                let data =
                    TrackedPcm::new(crate::mp3_decode::decode(resolved.bytes, || false).unwrap());
                let weak = Arc::downgrade(&data);
                let offset = delay.map(|d| {
                    (d.as_nanos() * data.pcm.rate as u128 / 1_000_000_000) as usize
                        * data.pcm.channels as usize
                });
                if repeat == 2 && entry.id != "builtin:school-bell" {
                    assert_eq!(
                        delay.unwrap().as_millis(),
                        if entry.id == "builtin:cuckoo-clock" {
                            1367
                        } else {
                            2000
                        }
                    );
                }
                let calls = Arc::new(AtomicUsize::new(0));
                let counter = calls.clone();
                let source = TimedPcm::new(data.clone(), delay, move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                })
                .unwrap();
                let output: Vec<_> = source.collect();
                assert_eq!(output.len(), data.pcm.samples.len() + offset.unwrap_or(0));
                for (i, actual) in output.iter().enumerate() {
                    let expected = data.pcm.samples.get(i).copied().unwrap_or(0.)
                        + offset
                            .and_then(|o| i.checked_sub(o))
                            .and_then(|j| data.pcm.samples.get(j))
                            .copied()
                            .unwrap_or(0.);
                    assert_eq!(*actual, expected);
                    assert!(actual.abs() < 1.);
                }
                assert_eq!(calls.load(Ordering::SeqCst), usize::from(delay.is_some()));
                drop(data);
                assert!(weak.upgrade().is_none());
            }
        }
        let long = crate::mp3_decode::decode(SCHOOL_LONG.bytes, || false).unwrap();
        assert_eq!(long.samples.len() / long.channels as usize, 514944);
        assert!(crate::mp3_decode::decode(
            &SCHOOL_LONG.bytes[..SCHOOL_LONG.bytes.len() - 1],
            || false
        )
        .is_err());
    }
    #[test]
    fn dropping_an_active_mix_releases_both_voices_and_pending_callback() {
        let data = TrackedPcm::new(crate::mp3_decode::decode(ENTRIES[1].bytes, || false).unwrap());
        let weak = Arc::downgrade(&data);
        let mut source = TimedPcm::new(data.clone(), Some(Duration::from_secs(2)), || {}).unwrap();
        for _ in 0..(44100 * 2 * 3) {
            assert!(source.next().is_some());
        }
        drop(data);
        assert!(weak.upgrade().is_some());
        drop(source);
        assert!(weak.upgrade().is_none());
    }
    #[test]
    fn four_original_mp3s_decode_fully_and_match_manifest() {
        let manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../assets/sounds/builtin_mp3/manifest.json"
        ))
        .unwrap();
        assert_eq!(ENTRIES.len(), 4);
        for entry in &ENTRIES {
            let row = manifest
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["id"] == entry.id)
                .unwrap();
            assert_eq!(
                format!("{:x}", Sha256::digest(entry.bytes)),
                row["sha256"].as_str().unwrap()
            );
            let pcm = crate::mp3_decode::decode(entry.bytes, || false).unwrap();
            assert_eq!(pcm.rate, 44100);
            assert_eq!(pcm.channels, 2);
            assert_eq!(
                (pcm.samples.len() / 2) as u64,
                row["sampleFrames"].as_u64().unwrap()
            );
            assert!(pcm.samples.iter().all(|s| s.is_finite()));
            assert!(pcm.packets > 0 && pcm.mpeg_frames >= pcm.packets);
        }
    }
    #[test]
    fn truncation_bad_frame_and_decode_cancellation_are_errors() {
        for entry in &ENTRIES {
            assert!(
                crate::mp3_decode::decode(&entry.bytes[..entry.bytes.len() - 1], || false).is_err()
            );
            assert!(crate::mp3_decode::decode(entry.bytes, || true).is_err());
        }
        assert!(crate::mp3_decode::decode(b"not an mp3", || false).is_err());
    }
    #[test]
    fn repeated_sources_share_one_pcm_allocation_and_release_it() {
        let pcm = TrackedPcm::new(crate::mp3_decode::decode(ENTRIES[1].bytes, || false).unwrap());
        let weak = Arc::downgrade(&pcm);
        let one = SharedPcm::new(pcm.clone());
        let two = SharedPcm::new(pcm.clone());
        assert!(Arc::ptr_eq(&one.data, &two.data));
        assert_eq!(Arc::strong_count(&pcm), 3);
        assert_eq!(one.collect::<Vec<_>>(), two.collect::<Vec<_>>());
        drop(pcm);
        assert!(weak.upgrade().is_none());
    }
    #[test]
    fn unknown_ids_and_paths_are_not_audio_sources() {
        for id in [
            "../x.mp3",
            "file:///C:/dummy.mp3",
            "user:dummy",
            "builtin:unknown",
            "cuckoo-clock.mp3",
        ] {
            assert!(find(id).is_none());
            assert_eq!(
                crate::audio::Audio::default()
                    .submit_builtin(false, 1, 0.5, None, None, id)
                    .unwrap_err(),
                "UnknownBuiltin"
            );
        }
    }
    #[test]
    fn invalid_side_information_is_a_decode_error_not_clean_eof() {
        let mut bytes = ENTRIES[1].bytes.to_vec();
        let first = bytes
            .windows(4)
            .position(|b| b[0] == 255 && b[1] & 224 == 224 && ((b[1] >> 1) & 3) == 1)
            .unwrap();
        let b = &bytes[first..];
        let rate = [44100usize, 48000, 32000][((b[2] >> 2) & 3) as usize];
        let kbps = [
            0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
        ][(b[2] >> 4) as usize];
        let second = first + 144000 * kbps / rate + ((b[2] >> 1) & 1) as usize;
        bytes[second + 4..second + 36].fill(255);
        assert!(crate::mp3_decode::frames(&bytes).is_ok());
        let error = crate::mp3_decode::decode(&bytes, || false)
            .err()
            .expect("corrupt side-info must fail");
        assert!(error.starts_with("DecodeError:"), "{error}");
    }
}
