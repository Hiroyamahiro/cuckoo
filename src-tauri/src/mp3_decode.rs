use std::{io::Cursor, time::Duration};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{DecoderOptions, CODEC_TYPE_MP3},
    errors::Error,
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};
pub struct Pcm {
    pub samples: Vec<f32>,
    pub rate: u32,
    pub channels: u16,
    pub packets: usize,
    pub mpeg_frames: usize,
    pub work_buffer_bytes: usize,
}
impl Pcm {
    pub fn duration(&self) -> Duration {
        Duration::from_secs_f64(self.samples.len() as f64 / self.channels as f64 / self.rate as f64)
    }
}
// Strict framing prevents a truncated last packet from being accepted as clean EOF.
pub fn frames(bytes: &[u8]) -> Result<usize, String> {
    let (mut pos, mut end) = (0usize, bytes.len());
    if bytes.starts_with(b"ID3") {
        if bytes.len() < 10
            || ![2, 3, 4].contains(&bytes[3])
            || bytes[6..10].iter().any(|v| v & 128 != 0)
        {
            return Err("InvalidId3".into());
        }
        let n = bytes[6..10]
            .iter()
            .fold(0usize, |n, v| (n << 7) | *v as usize);
        pos = 10
            + n
            + if bytes[3] == 4 && bytes[5] & 16 != 0 {
                10
            } else {
                0
            };
        if pos > end {
            return Err("TruncatedId3".into());
        }
    }
    if end >= 128 && &bytes[end - 128..end - 125] == b"TAG" {
        end -= 128;
    }
    let (mut count, mut format) = (0, None);
    while pos < end {
        if pos + 4 > end {
            return Err("TruncatedMp3Header".into());
        }
        let b = &bytes[pos..];
        let version = (b[1] >> 3) & 3;
        let layer = (b[1] >> 1) & 3;
        let bitrate = (b[2] >> 4) as usize;
        let rate_index = ((b[2] >> 2) & 3) as usize;
        if b[0] != 255
            || b[1] & 224 != 224
            || version == 1
            || layer != 1
            || bitrate == 0
            || bitrate == 15
            || rate_index == 3
        {
            return Err(format!("InvalidMp3Frame:{pos}"));
        }
        let rate = [44100usize, 48000, 32000][rate_index]
            / match version {
                3 => 1,
                2 => 2,
                _ => 4,
            };
        let channels = if b[3] >> 6 == 3 { 1 } else { 2 };
        let current = (rate, channels);
        if format.is_some_and(|f| f != current) {
            return Err("ChangingMp3Format".into());
        }
        format = Some(current);
        let kbps = if version == 3 {
            [
                0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
            ][bitrate]
        } else {
            [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160][bitrate]
        };
        let length =
            if version == 3 { 144000 } else { 72000 } * kbps / rate + ((b[2] >> 1) & 1) as usize;
        if length < 4 || pos + length > end {
            return Err(format!("TruncatedMp3Frame:{pos}"));
        }
        pos += length;
        count += 1;
    }
    if count == 0 {
        return Err("NoMp3Frames".into());
    }
    Ok(count)
}
pub fn decode(bytes: &[u8], mut cancelled: impl FnMut() -> bool) -> Result<Pcm, String> {
    if bytes.len() > 26214400 {
        return Err("Mp3TooLarge".into());
    }
    let mpeg_frames = frames(bytes)?;
    let stream = MediaSourceStream::new(Box::new(Cursor::new(bytes.to_vec())), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    // Preserve all decoded samples; no silence trimming, gain change, or gapless trimming.
    let mut format = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions {
                enable_gapless: false,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )
        .map_err(|e| e.to_string())?
        .format;
    let track = format.default_track().ok_or("NoAudioTrack")?;
    if track.codec_params.codec != CODEC_TYPE_MP3 {
        return Err("NotMp3".into());
    }
    let id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions { verify: true })
        .map_err(|e| e.to_string())?;
    let (mut samples, mut spec, mut buffer, mut packets, mut work_buffer_bytes) =
        (Vec::new(), None, None, 0, 0);
    loop {
        if cancelled() {
            return Err("DecodeCancelled".into());
        }
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("PacketError:{e}")),
        };
        if packet.track_id() != id {
            return Err("UnexpectedTrack".into());
        }
        let decoded = decoder
            .decode(&packet)
            .map_err(|e| format!("DecodeError:{e}"))?;
        let current = *decoded.spec();
        let channels = current.channels.count();
        if channels == 0 || channels > 2 || ![44100, 48000].contains(&current.rate) {
            return Err("UnsupportedPcmFormat".into());
        }
        if spec.is_some_and(|s| s != current) {
            return Err("ChangingPcmFormat".into());
        }
        spec = Some(current);
        let target = (current.rate as usize) * channels * 60;
        if samples.len() + decoded.frames() * channels > target {
            return Err("Mp3TooLong".into());
        }
        let output = buffer
            .get_or_insert_with(|| SampleBuffer::<f32>::new(decoded.capacity() as u64, current));
        output.copy_interleaved_ref(decoded);
        if output.samples().iter().any(|v| !v.is_finite()) {
            return Err("NonFinitePcm".into());
        }
        work_buffer_bytes = work_buffer_bytes.max(output.capacity() * std::mem::size_of::<f32>());
        samples.extend_from_slice(output.samples());
        packets += 1;
    }
    if decoder.finalize().verify_ok == Some(false) {
        return Err("DecodeVerificationFailed".into());
    }
    let spec = spec.ok_or("EmptyDecodedAudio")?;
    if samples.is_empty() {
        return Err("EmptyDecodedAudio".into());
    }
    Ok(Pcm {
        samples,
        rate: spec.rate,
        channels: spec.channels.count() as u16,
        packets,
        mpeg_frames,
        work_buffer_bytes,
    })
}
