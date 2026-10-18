//! Generate on a blocking worker; immutable WAVs feed the existing audio worker.
use crate::sound_library::paths;
#[path = "voicevox_cache.rs"]
mod cache;
#[path = "voicevox_images.rs"]
mod images;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant, SystemTime},
};
const MAX_WAV: usize = 12 * 1024 * 1024;
const MAX_CACHE: u64 = 1024 * 1024 * 1024;
static GENERATION: Mutex<()> = Mutex::new(());
// Short handoff protection; explicit editor leases cover the full editing lifetime.
static RECENT: std::sync::LazyLock<Mutex<HashMap<String, Instant>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Speech {
    pub character: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<String>,
    pub mode: String,
    pub text: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RouteSpeech {
    pub settings: Speech,
    pub clips: std::collections::BTreeMap<u16, String>,
}
impl RouteSpeech {
    pub fn validate(&self, offset: u16) -> Result<(), String> {
        if self.clips.len() != 24 {
            return Err("全24時刻の読み上げ音声を生成してください。".into());
        }
        for hour in 0..24 {
            let minute = hour * 60 + offset;
            text(&self.settings, minute)?;
            if !self.clips.get(&minute).is_some_and(|id| id_valid(id)) {
                return Err("MissingRouteSpeech".into());
            }
        }
        Ok(())
    }
    pub fn validate_cache(&self, root: &Path) -> Result<(), String> {
        for (minute, id) in &self.clips {
            validate_cached(root, id, &self.settings, *minute)?;
        }
        Ok(())
    }
}
#[derive(Deserialize)]
struct Character {
    id: String,
    name: String,
    style: String,
}
fn characters() -> Vec<Character> {
    serde_json::from_str(include_str!("../../src/shared/voicevox-legacy.json"))
        .expect("bundled characters")
}
pub fn text(s: &Speech, minute: u16) -> Result<String, String> {
    if minute >= 1440
        || !(if s.style_id.is_some() {
            uuid::Uuid::parse_str(&s.character).is_ok_and(|u| u.to_string() == s.character)
        } else {
            characters().iter().any(|c| c.id == s.character)
        })
        || s.character_name
            .as_ref()
            .is_some_and(|v| v.chars().count() > 200)
        || s.style_name
            .as_ref()
            .is_some_and(|v| v.chars().count() > 200)
        || s.policy.as_ref().is_some_and(|v| v.len() > 65536)
        || !["time", "time-text", "text"].contains(&s.mode.as_str())
        || s.text.chars().count() > 500
        || s.text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
    {
        return Err("読み上げ設定が不正です。文章は500文字以内で入力してください。".into());
    }
    let time = if minute % 60 == 0 {
        format!("{}時です。", minute / 60)
    } else {
        format!("{}時{}分です。", minute / 60, minute % 60)
    };
    match s.mode.as_str() {
        "time" => Ok(time),
        "time-text" | "text" if s.text.trim().is_empty() => {
            Err("自由セリフを入力してください。".into())
        }
        "time-text" => Ok(format!("{time}{}", s.text.trim())),
        _ => Ok(s.text.trim().into()),
    }
}
fn hash(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
fn key(s: &Speech, minute: u16) -> Result<String, String> {
    if let Some(style) = s.style_id {
        return Ok(hash(
            &serde_json::to_vec(&json!([
                "voicevox-v2",
                s.character,
                style,
                text(s, minute)?,
                48000,
                false
            ]))
            .unwrap(),
        ));
    }
    Ok(hash(
        &serde_json::to_vec(&json!([
            "voicevox-v1",
            s.character,
            text(s, minute)?,
            48000,
            false
        ]))
        .unwrap(),
    ))
}
pub fn id_valid(id: &str) -> bool {
    id.strip_prefix("voicevox:").is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
pub fn playback_repeat(id: &str, repeat: u64) -> u64 {
    if id_valid(id) {
        1
    } else {
        repeat
    }
}
pub fn root() -> Result<PathBuf, String> {
    #[cfg(feature = "settings-test-harness")]
    {
        Ok(crate::settings_disk::verification_root())
    }
    #[cfg(not(feature = "settings-test-harness"))]
    {
        crate::settings_disk::production_path()
            .map(|p| p.parent().unwrap().to_path_buf())
            .map_err(|e| e.code)
    }
}
fn path(root: &Path, id: &str, ext: &str) -> Result<PathBuf, String> {
    if !id_valid(id) {
        return Err("InvalidVoicevoxId".into());
    }
    Ok(root.join("voicevox").join(format!("{}.{ext}", &id[9..])))
}
fn io(e: crate::sound_library::Error) -> String {
    format!("音声キャッシュにアクセスできません（{}）。", e.code)
}
pub fn hold(root: &Path, id: &str) -> Result<File, String> {
    let mut f = paths::open_read(&path(root, id, "wav")?).map_err(io)?;
    let mut b = Vec::new();
    (&mut f)
        .take(MAX_WAV as u64 + 1)
        .read_to_end(&mut b)
        .map_err(|e| e.to_string())?;
    let meta = paths::read(&path(root, id, "json")?, 16384)
        .map_err(io)?
        .ok_or("MissingVoicevoxMetadata")?;
    let meta: Value = serde_json::from_slice(&meta).map_err(|_| "InvalidVoicevoxMetadata")?;
    let digest = hash(&b);
    let identity = hash(
        format!(
            "{}:{digest}",
            meta["key"].as_str().ok_or("InvalidVoicevoxKey")?
        )
        .as_bytes(),
    );
    if b.len() > MAX_WAV || meta["wavSha256"] != digest || identity != id[9..] {
        return Err("生成音声が破損しています。時報を編集して再生成してください。".into());
    }
    decode(&b)?;
    Ok(f)
}
pub fn validate_cached(root: &Path, id: &str, s: &Speech, minute: u16) -> Result<(), String> {
    let meta = paths::read(&path(root, id, "json")?, 16384)
        .map_err(io)?
        .ok_or("生成音声の情報がありません。再生成してください。")?;
    let meta: Value = serde_json::from_slice(&meta).map_err(|_| "InvalidVoicevoxMetadata")?;
    if meta["key"] != key(s, minute)? || meta["soundId"] != id {
        return Err("読み上げ設定が変更されています。音声を生成してから保存してください。".into());
    }
    hold(root, id)?;
    Ok(())
}
pub fn decode(b: &[u8]) -> Result<crate::mp3_decode::Pcm, String> {
    let fail =
        || "VOICEVOX音声は60秒以内のPCM WAV（48kHz・16bit・モノラル）にしてください。".to_string();
    if b.len() < 44
        || b.len() > MAX_WAV
        || &b[..4] != b"RIFF"
        || &b[8..12] != b"WAVE"
        || u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize + 8 != b.len()
    {
        return Err(fail());
    }
    let mut pos = 12;
    let mut fmt = None;
    let mut data = None;
    while pos + 8 <= b.len() {
        let len = u32::from_le_bytes(b[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let end = (pos + 8)
            .checked_add(len)
            .filter(|e| *e <= b.len())
            .ok_or_else(fail)?;
        let body = &b[pos + 8..end];
        match &b[pos..pos + 4] {
            b"fmt " => {
                if fmt.is_some() || len < 16 {
                    return Err(fail());
                }
                fmt = Some(body)
            }
            b"data" => {
                if data.is_some() {
                    return Err(fail());
                }
                data = Some(body)
            }
            _ => {}
        }
        pos = end + (len % 2);
    }
    let f = fmt.ok_or_else(fail)?;
    let d = data.ok_or_else(fail)?;
    let u16at = |i| u16::from_le_bytes(f[i..i + 2].try_into().unwrap());
    if pos != b.len()
        || u16at(0) != 1
        || u16at(2) != 1
        || u32::from_le_bytes(f[4..8].try_into().unwrap()) != 48000
        || u16at(12) != 2
        || u16at(14) != 16
        || d.is_empty()
        || d.len() % 2 != 0
        || d.len() > 48000 * 2 * 60
    {
        return Err(fail());
    }
    Ok(crate::mp3_decode::Pcm {
        samples: d
            .chunks_exact(2)
            .map(|p| i16::from_le_bytes([p[0], p[1]]) as f32 / 32768.)
            .collect(),
        rate: 48000,
        channels: 1,
        packets: 1,
        mpeg_frames: 0,
        work_buffer_bytes: 0,
    })
}
pub struct PlaybackPin {
    _file: File,
    _use: cache::Playing,
}
pub fn read_pcm(id: &str) -> Result<(PlaybackPin, crate::mp3_decode::Pcm), String> {
    let root = root()?;
    let active = cache::playing(&root, id)?;
    let f = hold(&root, id)?;
    let b = paths::read(&path(&root, id, "wav")?, MAX_WAV as u64)
        .map_err(io)?
        .ok_or("MissingVoicevoxAudio")?;
    Ok((
        PlaybackPin {
            _file: f,
            _use: active,
        },
        decode(&b)?,
    ))
}
fn json_response(bytes: &[u8]) -> Result<Value, String> {
    serde_json::from_slice(bytes).map_err(|_| {
        "VOICEVOXの応答形式を確認できません。対応するエンジンを起動してください。".into()
    })
}
fn api_json(method: &str, route: &str, body: &[u8]) -> Result<Value, String> {
    json_response(&crate::voicevox_http::request(
        method,
        route,
        body,
        1024 * 1024,
    )?)
}
pub fn check() -> Result<Value, String> {
    let version = api_json("GET", "/version", &[])?;
    let speakers = api_json("GET", "/speakers", &[])?;
    Ok(json!({"version":version,"characters":speaker_list(&speakers)?}))
}
fn speaker_list(speakers: &Value) -> Result<Vec<Value>, String> {
    let list = speakers.as_array().ok_or("InvalidSpeakers")?;
    if list.len() > 2000 {
        return Err("InvalidSpeakers".into());
    }
    let mut result = Vec::new();
    let mut uuids = HashSet::new();
    for speaker in list {
        let Some(uuid) = speaker["speaker_uuid"]
            .as_str()
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
            .map(|u| u.to_string())
        else {
            continue;
        };
        let Some(name) = speaker["name"]
            .as_str()
            .filter(|s| !s.is_empty() && s.chars().count() <= 200)
        else {
            continue;
        };
        let Some(styles) = speaker["styles"].as_array() else {
            continue;
        };
        let mut ids = HashSet::new();
        let styles: Vec<Value> = styles
            .iter()
            .filter_map(|style| {
                if !style
                    .get("type")
                    .is_none_or(|t| t == "talk" || t == "streaming_talk")
                {
                    return None;
                }
                let id = u32::try_from(style["id"].as_u64()?).ok()?;
                let name = style["name"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.chars().count() <= 200)?;
                ids.insert(id).then(
                    || json!({"id":id,"name":name,"type":style["type"].as_str().unwrap_or("talk")}),
                )
            })
            .collect();
        if !styles.is_empty() && uuids.insert(uuid.clone()) {
            result.push(json!({"uuid":uuid,"name":name,"styles":styles,"version":speaker["version"].as_str().unwrap_or("")}));
        }
    }
    Ok(result)
}
fn style(speakers: &Value, s: &Speech) -> Result<(u64, String, String), String> {
    let legacy = characters().into_iter().find(|c| c.id == s.character);
    for speaker in speakers.as_array().ok_or("InvalidSpeakers")? {
        if if s.style_id.is_some() {
            speaker["speaker_uuid"] == s.character
        } else {
            legacy.as_ref().is_some_and(|c| speaker["name"] == c.name)
        } {
            for style in speaker["styles"].as_array().ok_or("InvalidStyles")? {
                if (if let Some(id) = s.style_id {
                    style["id"].as_u64() == Some(id as u64)
                } else {
                    legacy.as_ref().is_some_and(|c| style["name"] == c.style)
                }) && style
                    .get("type")
                    .is_none_or(|t| t == "talk" || t == "streaming_talk")
                {
                    return Ok((
                        style["id"].as_u64().ok_or("InvalidStyleId")?,
                        speaker["speaker_uuid"]
                            .as_str()
                            .ok_or("InvalidSpeakerUuid")?
                            .into(),
                        speaker["version"].as_str().unwrap_or("").into(),
                    ));
                }
            }
        }
    }
    Err("選択したキャラクター・スタイルが見つかりません。一覧を更新して再選択してください。生成済み音声は引き続き利用できます。".into())
}
#[tauri::command]
pub async fn voicevox_thumbnail(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    uuid: String,
    style_id: u32,
) -> Result<String, String> {
    crate::current_window(&app, &window)?;
    tauri::async_runtime::spawn_blocking(move || {
        images::thumbnail(&uuid, style_id, &mut crate::voicevox_http::request)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn voicevox_policy(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    uuid: String,
) -> Result<String, String> {
    crate::current_window(&app, &window)?;
    tauri::async_runtime::spawn_blocking(move || {
        let uuid = uuid::Uuid::parse_str(&uuid).map_err(|_| "InvalidSpeakerUuid")?;
        let route = format!("/speaker_info?speaker_uuid={uuid}&resource_format=url");
        // URL resource format avoids downloading portraits and voice samples.
        let value = json_response(&crate::voicevox_http::request(
            "GET",
            &route,
            &[],
            4 * 1024 * 1024,
        )?)?;
        let policy = value["policy"]
            .as_str()
            .ok_or("音声ライブラリの利用規約を取得できません。")?;
        if policy.trim().is_empty() {
            return Err("音声ライブラリの利用規約を取得できません。".into());
        }
        if policy.len() > 65536 {
            return Err("音声ライブラリの利用規約が大きすぎます。".into());
        }
        Ok(policy.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
fn publish(dir: &Path, name: &str, b: &[u8]) -> Result<(), String> {
    let target = dir.join(name);
    paths::safe(&target).map_err(io)?;
    if target.exists() {
        let file = paths::open_read(&target).map_err(io)?;
        let size = file.metadata().map_err(|e| e.to_string())?.len();
        drop(file);
        let old = if size <= MAX_WAV as u64 {
            paths::read(&target, MAX_WAV as u64).map_err(io)?
        } else {
            None
        };
        if old.as_deref() == Some(b) {
            return Ok(());
        }
    }
    let temp = dir.join(format!("{}.tmp", uuid::Uuid::new_v4()));
    paths::write_new(&temp, b).map_err(io)?;
    // Verified incoming data has a content-derived name. Replace a corrupt copy
    // atomically, never truncate an existing file (playback pins prevent replacement).
    let wide = |p: &Path| {
        p.to_string_lossy()
            .encode_utf16()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    let result = if unsafe {
        windows_sys::Win32::Storage::FileSystem::MoveFileExW(
            wide(&temp).as_ptr(),
            wide(&target).as_ptr(),
            windows_sys::Win32::Storage::FileSystem::MOVEFILE_REPLACE_EXISTING
                | windows_sys::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err("音声キャッシュを更新できません。試聴の終了後に再試行してください。".into())
    } else {
        Ok(())
    };
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
fn cleanup(root: &Path) -> Result<(), String> {
    cache::cleanup_at(root, SystemTime::now()).map(|_| ())
}
pub(crate) fn request_cleanup(root: PathBuf) {
    cache::request(root);
}
pub(crate) fn reference_guard() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    cache::REFERENCES
        .lock()
        .map_err(|_| "VoicevoxReferencesPoisoned".into())
}
pub fn generate(s: Speech, minute: u16, force: bool) -> Result<Value, String> {
    generate_with(&root()?, s, minute, force)
}
fn generate_with(root: &Path, s: Speech, minute: u16, force: bool) -> Result<Value, String> {
    generate_using(root, s, minute, force, &mut crate::voicevox_http::request)
}
fn generate_using(
    root: &Path,
    s: Speech,
    minute: u16,
    force: bool,
    request: &mut impl FnMut(&str, &str, &[u8], usize) -> Result<Vec<u8>, String>,
) -> Result<Value, String> {
    let _lock = GENERATION
        .try_lock()
        .map_err(|_| "音声を生成中です。完了してから再試行してください。")?;
    let recipe = key(&s, minute)?;
    let dir = root.join("voicevox");
    let _parents = paths::directory_chain(&dir).map_err(io)?;
    // Broken app-named metadata is isolated; unrelated entries cannot block reuse.
    if !force {
        for entry in fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let Ok(entry) = entry else { continue };
            let Some(v) = cache::metadata(&entry.path()) else {
                continue;
            };
            if v["key"] == recipe {
                if let Some(id) = v["soundId"].as_str() {
                    if validate_cached(root, id, &s, minute).is_ok() {
                        RECENT
                            .lock()
                            .map_err(|_| "VoicevoxCacheBusy")?
                            .insert(id.into(), Instant::now());
                        return Ok(json!({"soundId":id,"cached":true}));
                    }
                }
            }
        }
    }
    let version = json_response(&request("GET", "/version", &[], 1024 * 1024)?)?;
    let speakers = json_response(&request("GET", "/speakers", &[], 1024 * 1024)?)?;
    let (speaker, uuid, model) = style(&speakers, &s)?;
    let mut query_url = tauri::Url::parse("http://127.0.0.1:50021/audio_query").unwrap();
    query_url
        .query_pairs_mut()
        .append_pair("speaker", &speaker.to_string())
        .append_pair("text", &text(&s, minute)?);
    let mut query = json_response(&request(
        "POST",
        &format!("{}?{}", query_url.path(), query_url.query().unwrap()),
        &[],
        1024 * 1024,
    )?)?;
    if !query.is_object() {
        return Err("InvalidAudioQuery".into());
    }
    query["outputSamplingRate"] = json!(48000);
    query["outputStereo"] = json!(false);
    query["volumeScale"] = json!(1.0);
    let wav = request(
        "POST",
        &format!("/synthesis?speaker={speaker}"),
        &serde_json::to_vec(&query).map_err(|e| e.to_string())?,
        MAX_WAV,
    )?;
    decode(&wav)?;
    // Unknown/locked backups stop deletion, but must not prevent new generation.
    if let Err(e) = cleanup(root) {
        eprintln!("VOICEVOX cache cleanup skipped: {e}");
    }
    let wav_hash = hash(&wav);
    let digest = hash(format!("{recipe}:{wav_hash}").as_bytes());
    let id = format!("voicevox:{digest}");
    let meta = json!({"key":recipe,"soundId":id,"wavSha256":wav_hash,"engineVersion":version,"speakerUuid":uuid,"speakerVersion":model});
    let meta_bytes = serde_json::to_vec(&meta).map_err(|e| e.to_string())?;
    cache::capacity(
        &dir,
        &[
            (format!("{digest}.wav"), wav.len() as u64),
            (format!("{digest}.json"), meta_bytes.len() as u64),
        ],
        MAX_CACHE,
    )?;
    publish(&dir, &format!("{digest}.wav"), &wav)?;
    publish(&dir, &format!("{digest}.json"), &meta_bytes)?;
    validate_cached(root, &id, &s, minute)?;
    RECENT
        .lock()
        .map_err(|_| "VoicevoxCacheBusy")?
        .insert(id.clone(), Instant::now());
    Ok(json!({"soundId":id,"cached":false}))
}
#[tauri::command]
pub async fn voicevox_check(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Value, String> {
    crate::current_window(&app, &window)?;
    tauri::async_runtime::spawn_blocking(check)
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn voicevox_protect_editor(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    session: String,
    ids: Vec<String>,
) -> Result<(), String> {
    crate::current_window(&app, &window)?;
    tauri::async_runtime::spawn_blocking(move || {
        let root = root()?;
        let released = ids.is_empty();
        cache::editor(&root, &session, ids)?;
        if released {
            request_cleanup(root);
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn voicevox_generate(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    speech: Speech,
    minute: u16,
    force: bool,
) -> Result<Value, String> {
    crate::current_window(&app, &window)?;
    tauri::async_runtime::spawn_blocking(move || generate(speech, minute, force))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            Self(
                std::env::temp_dir().join(format!("cuckoo-voicevox-test-{}", uuid::Uuid::new_v4())),
            )
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn speech(mode: &str) -> Speech {
        Speech {
            character: "zundamon".into(),
            style_id: None,
            character_name: None,
            style_name: None,
            policy: None,
            mode: mode.into(),
            text: if mode == "time" {
                String::new()
            } else {
                "そろそろ休憩するのだ！".into()
            },
        }
    }
    fn wav() -> Vec<u8> {
        let len = 4800u32 * 2;
        let mut b = Vec::new();
        b.extend(b"RIFF");
        b.extend((len + 36).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(48000u32.to_le_bytes());
        b.extend(96000u32.to_le_bytes());
        b.extend(2u16.to_le_bytes());
        b.extend(16u16.to_le_bytes());
        b.extend(b"data");
        b.extend(len.to_le_bytes());
        b.resize(len as usize + 44, 0);
        b
    }
    fn engine(method: &str, route: &str, body: &[u8], _: usize) -> Result<Vec<u8>, String> {
        Ok(if route == "/version" {
            br#""test-engine""#.to_vec()
        } else if route == "/speakers" {
            serde_json::to_vec(&json!([{"name":"ずんだもん","speaker_uuid":"z","version":"1","styles":[{"id":987,"name":"ノーマル","type":"talk"}]}])).unwrap()
        } else if route.starts_with("/audio_query?") {
            assert_eq!(method, "POST");
            let u = tauri::Url::parse(&format!("http://localhost{route}")).unwrap();
            let pairs: std::collections::HashMap<_, _> = u.query_pairs().collect();
            assert_eq!(pairs["speaker"], "987");
            assert!(pairs["text"].contains("です。"));
            b"{}".to_vec()
        } else if route == "/synthesis?speaker=987" {
            let q: Value = serde_json::from_slice(body).unwrap();
            assert_eq!(q["volumeScale"], 1.0);
            assert_eq!(q["outputSamplingRate"], 48000);
            assert_eq!(q["outputStereo"], false);
            wav()
        } else {
            panic!("unexpected route {route}")
        })
    }
    #[test]
    fn all_minutes_modes_and_cache_identity() {
        for minute in 0..1440 {
            let t = text(&speech("time"), minute).unwrap();
            assert!(t.starts_with(&format!("{}時", minute / 60)));
            assert_eq!(t.contains('分'), minute % 60 != 0);
        }
        assert_eq!(text(&speech("time"), 600).unwrap(), "10時です。");
        assert_eq!(
            text(&speech("time-text"), 635).unwrap(),
            "10時35分です。そろそろ休憩するのだ！"
        );
        assert_eq!(
            key(&speech("text"), 0).unwrap(),
            key(&speech("text"), 1439).unwrap()
        );
        assert_ne!(
            key(&speech("time"), 0).unwrap(),
            key(&speech("time"), 1).unwrap()
        );
        let mut bad = speech("text");
        bad.text.clear();
        assert!(text(&bad, 0).is_err());
        bad.text = "あ".repeat(501);
        assert!(text(&bad, 0).is_err());
        assert!(text(&speech("time"), 1440).is_err());
        assert!(!id_valid("voicevox:../../bad"));
        assert!(!id_valid(&format!("voicevox:{}", "G".repeat(64))));
    }
    #[test]
    fn generation_cache_offline_failure_preservation_and_persistence() {
        let temp = Temp::new();
        let first = generate_using(&temp.0, speech("time-text"), 635, false, &mut engine).unwrap();
        let id = first["soundId"].as_str().unwrap();
        assert_eq!(first["cached"], false);
        let mut offline = |_: &str, _: &str, _: &[u8], _: usize| Err("offline".to_string());
        assert_eq!(
            generate_using(&temp.0, speech("time-text"), 635, false, &mut offline).unwrap()
                ["cached"],
            true
        );
        assert!(generate_using(&temp.0, speech("time-text"), 635, true, &mut offline).is_err());
        assert!(generate_using(&temp.0, speech("time-text"), 636, false, &mut offline).is_err());
        validate_cached(&temp.0, id, &speech("time-text"), 635).unwrap();
        assert!(validate_cached(&temp.0, id, &speech("time-text"), 636).is_err());
        fs::write(path(&temp.0, id, "wav").unwrap(), b"broken").unwrap();
        assert!(hold(&temp.0, id).is_err());
        assert_eq!(
            generate_using(&temp.0, speech("time-text"), 635, true, &mut engine).unwrap()
                ["soundId"],
            id
        );
        validate_cached(&temp.0, id, &speech("time-text"), 635).unwrap();
        // Preview/save use the non-forced lookup and must repair corrupt payloads too.
        let wav_path = path(&temp.0, id, "wav").unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&wav_path)
            .unwrap()
            .set_len(MAX_WAV as u64 + 1)
            .unwrap();
        fs::write(path(&temp.0, id, "json").unwrap(), vec![b'x'; 20_000]).unwrap();
        assert_eq!(
            generate_using(&temp.0, speech("time-text"), 635, false, &mut engine).unwrap()
                ["soundId"],
            id
        );
        validate_cached(&temp.0, id, &speech("time-text"), 635).unwrap();
        // Same waveform with a different recipe has its own immutable identity.
        let other = generate_using(&temp.0, speech("time-text"), 636, false, &mut engine).unwrap();
        assert_ne!(first["soundId"], other["soundId"]);
        let legacy = crate::settings_model::Document::default();
        assert!(legacy.schedule.custom.is_empty());
        let changed=legacy.patched(&json!({"customChimes":[{"id":uuid::Uuid::new_v4().to_string(),"minute":635,"enabled":true,"soundId":id,"repeat":2,"volume":80,"speech":speech("time-text")}]})).unwrap();
        let mut store = crate::settings_disk::Store::test(temp.0.clone()).unwrap();
        store.load().unwrap();
        let saved = store.commit(changed).unwrap();
        drop(store);
        let mut store = crate::settings_disk::Store::test(temp.0.clone()).unwrap();
        let crate::settings_disk::Loaded::Valid(reloaded) = store.load().unwrap() else {
            panic!("not persisted")
        };
        assert_eq!(saved, reloaded);
        assert_eq!(
            decode(
                &paths::read(&path(&temp.0, id, "wav").unwrap(), MAX_WAV as u64)
                    .unwrap()
                    .unwrap()
            )
            .unwrap()
            .samples
            .len(),
            4800
        );
        // A changed time cannot save an old time-reading clip; the good settings survive.
        let changed=reloaded.patched(&json!({"customChimes":[{"id":reloaded.schedule.custom[0].id,"minute":636,"enabled":true,"soundId":id,"repeat":2,"volume":80,"speech":speech("time-text")}]})).unwrap();
        assert!(store.commit(changed).is_err());
        // A draft's cache hit is protected; a newly unreferenced clip gets a full week.
        cleanup(&temp.0).unwrap();
        assert!(path(&temp.0, other["soundId"].as_str().unwrap(), "wav")
            .unwrap()
            .exists());
        RECENT.lock().unwrap().clear();
        let observed = SystemTime::now();
        cache::cleanup_at(&temp.0, observed).unwrap();
        assert!(hold(&temp.0, id).is_ok());
        assert!(path(&temp.0, other["soundId"].as_str().unwrap(), "wav")
            .unwrap()
            .exists());
        cache::cleanup_at(&temp.0, observed + Duration::from_secs(7 * 86400)).unwrap();
        assert!(!path(&temp.0, other["soundId"].as_str().unwrap(), "wav")
            .unwrap()
            .exists());
        // Deleting a cache does not block unrelated changes or deleting the chime.
        fs::remove_file(path(&temp.0, id, "wav").unwrap()).unwrap();
        let changed = reloaded.patched(&json!({"volume":12})).unwrap();
        store.commit(changed).unwrap();
        let changed = reloaded.patched(&json!({"customChimes":[]})).unwrap();
        store.commit(changed).unwrap();
    }
    #[test]
    fn invalid_wav_and_missing_characters_are_rejected() {
        let bytes = wav();
        assert_eq!(decode(&bytes).unwrap().rate, 48000);
        for n in [0, 12, 43, bytes.len() - 1] {
            assert!(decode(&bytes[..n]).is_err())
        }
        let mut bad = bytes.clone();
        bad[22] = 2;
        assert!(decode(&bad).is_err());
        bad = bytes;
        bad[34] = 8;
        assert!(decode(&bad).is_err());
        assert!(style(&json!([]), &speech("time")).is_err());
    }
    #[test]
    fn known_streaming_speech_styles_are_selectable_but_singing_and_unknown_are_not() {
        let uuid = "11111111-1111-4111-8111-111111111111";
        let speakers = json!([{"speaker_uuid":uuid,"name":"voice","styles":[{"id":1,"name":"talk","type":"talk"},{"id":2,"name":"streaming","type":"streaming_talk"},{"id":3,"name":"sing","type":"sing"},{"id":4,"name":"future","type":"future"}]}]);
        let list = speaker_list(&speakers).unwrap();
        assert_eq!(list[0]["styles"].as_array().unwrap().len(), 2);
        assert_eq!(list[0]["styles"][1]["type"], "streaming_talk");
        let mut s = speech("time");
        s.character = uuid.into();
        s.style_id = Some(2);
        assert_eq!(style(&speakers, &s).unwrap().0, 2);
        s.style_id = Some(3);
        assert!(style(&speakers, &s).is_err());
        s.style_id = Some(4);
        assert!(style(&speakers, &s).is_err());
    }
    #[test]
    fn dynamic_styles_filter_and_cache_compatibility() {
        let uuid = "11111111-1111-4111-8111-111111111111";
        let speakers = json!([{"speaker_uuid":uuid,"name":"追加された声","version":"2","styles":[{"id":7,"name":"通常"},{"id":8,"name":"静か","type":"talk"},{"id":9,"name":"歌","type":"sing"},{"id":10,"name":"未知","type":"future"},{"id":11,"name":"null","type":null}]}]);
        let list = speaker_list(&speakers).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["styles"].as_array().unwrap().len(), 2);
        assert_eq!(speaker_list(&json!([])).unwrap().len(), 0);
        assert!(speaker_list(&json!({})).is_err());
        let old = speech("time");
        let old_key = hash(
            &serde_json::to_vec(&json!([
                "voicevox-v1",
                "zundamon",
                text(&old, 635).unwrap(),
                48000,
                false
            ]))
            .unwrap(),
        );
        assert_eq!(key(&old, 635).unwrap(), old_key);
        let mut s = old.clone();
        s.character = uuid.into();
        s.style_id = Some(7);
        s.character_name = Some("追加された声".into());
        assert_eq!(style(&speakers, &s).unwrap().0, 7);
        let key7 = key(&s, 635).unwrap();
        s.style_id = Some(8);
        assert_ne!(key7, key(&s, 635).unwrap());
        assert_eq!(style(&speakers, &s).unwrap().0, 8);
        s.style_id = Some(9);
        assert!(style(&speakers, &s).is_err());
        s.style_id = Some(8);
        s.character_name = Some("名称変更".into());
        s.policy = Some("新しい規約".into());
        let key8 = key(&s, 635).unwrap();
        s.policy = None;
        assert_eq!(key8, key(&s, 635).unwrap());
        let temp = Temp::new();
        let mut calls = 0;
        let mut engine = |_: &str, route: &str, _: &[u8], _: usize| {
            calls += 1;
            Ok(if route == "/version" {
                br#""mock""#.to_vec()
            } else if route == "/speakers" {
                serde_json::to_vec(&speakers).unwrap()
            } else if route.starts_with("/audio_query?") {
                assert!(route.contains("speaker=8"));
                b"{}".to_vec()
            } else {
                assert_eq!(route, "/synthesis?speaker=8");
                wav()
            })
        };
        let generated = generate_using(&temp.0, s.clone(), 635, false, &mut engine).unwrap();
        assert_eq!(calls, 4);
        let mut offline = |_: &str, _: &str, _: &[u8], _: usize| Err("offline".into());
        assert_eq!(
            generate_using(&temp.0, s.clone(), 635, false, &mut offline).unwrap()["cached"],
            true
        );
        validate_cached(&temp.0, generated["soundId"].as_str().unwrap(), &s, 635).unwrap();
        s.style_id = Some(7);
        assert!(generate_using(&temp.0, s, 635, false, &mut offline).is_err());
        assert_eq!(
            serde_json::from_value::<Speech>(serde_json::to_value(&old).unwrap()).unwrap(),
            old
        );
    }
    #[test]
    #[ignore = "requires a running external VOICEVOX Engine with both characters"]
    fn live_engine() {
        let connection = check().unwrap();
        let speakers = connection["characters"].as_array().unwrap();
        assert!(speakers.len() >= 2);
        let mut selections = Vec::new();
        for c in speakers.iter().take(2) {
            for style in c["styles"].as_array().unwrap().iter().take(2) {
                selections.push((c, style));
            }
        }
        for (c, style) in selections {
            let temp = Temp::new();
            let s = Speech {
                character: c["uuid"].as_str().unwrap().into(),
                style_id: Some(style["id"].as_u64().unwrap() as u32),
                character_name: Some(c["name"].as_str().unwrap().into()),
                style_name: Some(style["name"].as_str().unwrap().into()),
                policy: None,
                mode: "time".into(),
                text: String::new(),
            };
            let result = generate_with(&temp.0, s.clone(), 635, false).unwrap();
            validate_cached(&temp.0, result["soundId"].as_str().unwrap(), &s, 635).unwrap();
            let mut offline = |_: &str, _: &str, _: &[u8], _: usize| Err("offline".into());
            assert_eq!(
                generate_using(&temp.0, s, 635, false, &mut offline).unwrap()["cached"],
                true
            );
        }
    }
}
