//! Lifecycle bookkeeping for the existing VOICEVOX files, not a second catalog.
#![cfg_attr(test, allow(dead_code))]
use super::*;
use std::collections::BTreeMap;
use std::sync::{Condvar, LazyLock, OnceLock};
const WEEK: u64 = 7 * 86400;
const TEMP_AGE: u64 = 86400;
const STATE_LIMIT: u64 = 1024 * 1024;
pub(super) static REFERENCES: Mutex<()> = Mutex::new(());
#[derive(Default)]
struct Uses {
    editors: HashMap<(PathBuf, String), HashSet<String>>,
    playing: HashMap<(PathBuf, String), usize>,
}
static USES: LazyLock<Mutex<Uses>> = LazyLock::new(|| Mutex::new(Uses::default()));
pub(super) struct Playing {
    root: PathBuf,
    id: String,
}
impl Drop for Playing {
    fn drop(&mut self) {
        if let Ok(mut uses) = USES.lock() {
            let key = (self.root.clone(), self.id.clone());
            if let Some(n) = uses.playing.get_mut(&key) {
                *n -= 1;
                if *n == 0 {
                    uses.playing.remove(&key);
                }
            }
        }
    }
}
pub(super) fn playing(root: &Path, id: &str) -> Result<Playing, String> {
    if !id_valid(id) {
        return Err("InvalidVoicevoxId".into());
    }
    let mut uses = USES.lock().map_err(|_| "VoicevoxCacheBusy")?;
    *uses.playing.entry((root.into(), id.into())).or_default() += 1;
    Ok(Playing {
        root: root.into(),
        id: id.into(),
    })
}
pub(super) fn editor(root: &Path, session: &str, ids: Vec<String>) -> Result<(), String> {
    if uuid::Uuid::parse_str(session).is_err()
        || ids.len() > 48
        || ids.iter().any(|id| !id_valid(id))
    {
        return Err("InvalidVoicevoxLease".into());
    }
    let mut uses = USES.lock().map_err(|_| "VoicevoxCacheBusy")?;
    let key = (root.into(), session.into());
    if ids.is_empty() {
        uses.editors.remove(&key);
    } else {
        uses.editors.insert(key, ids.into_iter().collect());
    }
    Ok(())
}
fn protected(uses: &Uses, root: &Path, id: &str) -> bool {
    uses.playing.contains_key(&(root.into(), id.into()))
        || uses
            .editors
            .iter()
            .any(|((r, _), ids)| r == root && ids.contains(id))
}
fn recent(id: &str) -> bool {
    RECENT
        .lock()
        .map(|r| {
            r.get(id)
                .is_some_and(|t| t.elapsed() < Duration::from_secs(300))
        })
        .unwrap_or(true)
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    version: u32,
    observed: u64,
    unused: BTreeMap<String, u64>,
    #[serde(default)]
    debris: BTreeMap<String, u64>,
    checksum: String,
}
impl State {
    fn checksum(&self) -> String {
        hash(
            &serde_json::to_vec(&json!([
                self.version,
                self.observed,
                self.unused,
                self.debris
            ]))
            .unwrap(),
        )
    }
    fn valid(&self) -> bool {
        self.version == 1
            && self.unused.len() + self.debris.len() <= 20000
            && self
                .unused
                .iter()
                .all(|(id, t)| id_valid(id) && *t <= self.observed)
            && self
                .debris
                .iter()
                .all(|(name, t)| quarantine_name(name) && *t <= self.observed)
            && self.checksum == self.checksum()
    }
}
fn seconds(now: SystemTime) -> Result<u64, String> {
    now.duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|_| "InvalidCacheClock".into())
}
fn uuid_name(s: &str) -> bool {
    uuid::Uuid::parse_str(s).is_ok_and(|u| u.get_version_num() == 4 && u.to_string() == s)
}
fn cache_name(name: &str) -> Option<String> {
    let (stem, ext) = name.rsplit_once('.')?;
    if ext != "wav" && ext != "json" {
        return None;
    }
    let id = format!("voicevox:{stem}");
    id_valid(&id).then_some(id)
}
fn quarantine_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".corrupt") else {
        return false;
    };
    let Some((id, uuid)) = stem.split_once('.') else {
        return false;
    };
    (id == "gc-state" || id_valid(&format!("voicevox:{id}"))) && uuid_name(uuid)
}
fn quarantine(p: &Path) -> Result<(), String> {
    let f = paths::open_read(p).map_err(io)?;
    drop(f);
    let stem = p
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("InvalidCacheName")?;
    let target = p.with_file_name(format!("{stem}.{}.corrupt", uuid::Uuid::new_v4()));
    paths::safe(&target).map_err(io)?;
    fs::rename(p, target).map_err(|e| e.to_string())
}
// A malformed/oversized, but regular app-named JSON never stops another lookup.
pub(super) fn metadata(p: &Path) -> Option<Value> {
    let name = p.file_name()?.to_str()?;
    let id = cache_name(name)?;
    if p.extension()? != "json" {
        return None;
    }
    let value = paths::read(p, 16384)
        .ok()
        .flatten()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    let valid = value.as_ref().is_some_and(|v| {
        let Some(key) = v["key"].as_str() else {
            return false;
        };
        let Some(wav) = v["wavSha256"].as_str() else {
            return false;
        };
        id_valid(&format!("voicevox:{key}"))
            && id_valid(&format!("voicevox:{wav}"))
            && v["soundId"] == id
            && hash(format!("{key}:{wav}").as_bytes()) == id[9..]
    });
    if valid {
        return value;
    }
    // Only verified regular files are moved; unknown names/reparse points remain untouched.
    if let Some(root) = p.parent().and_then(Path::parent) {
        if let Ok(uses) = USES.lock() {
            if !protected(&uses, root, &id) {
                let _ = quarantine(p);
            }
        }
    }
    None
}
fn read_state(dir: &Path, now: u64) -> Result<(State, bool), String> {
    let path = dir.join("gc-state.json");
    match paths::read(&path, STATE_LIMIT) {
        Ok(None) => Ok((
            State {
                version: 1,
                observed: now,
                ..State::default()
            },
            false,
        )),
        Ok(Some(b)) => {
            if let Ok(state) = serde_json::from_slice::<State>(&b) {
                if state.valid() {
                    return Ok((state, false));
                }
            }
            quarantine(&path)?;
            Ok((
                State {
                    version: 1,
                    observed: now,
                    ..State::default()
                },
                true,
            ))
        }
        Err(e) => {
            // Oversized regular bookkeeping is recoverable; unsafe/unreadable files block GC.
            if e.code == "ReadLimit" {
                quarantine(&path)?;
                Ok((
                    State {
                        version: 1,
                        observed: now,
                        ..State::default()
                    },
                    true,
                ))
            } else {
                Err(io(e))
            }
        }
    }
}
fn references(root: &Path) -> Result<HashSet<String>, String> {
    let mut files = vec![root.join("settings.json")];
    let backups = root.join("backups");
    paths::safe(&backups).map_err(io)?;
    if backups.exists() {
        for e in fs::read_dir(backups).map_err(|e| e.to_string())? {
            let p = e.map_err(|e| e.to_string())?.path();
            if p.extension().is_some_and(|s| s == "json") {
                files.push(p)
            }
        }
    }
    let mut refs = HashSet::new();
    for p in files {
        let bytes = paths::read(&p, crate::settings_model::MAX_BYTES as u64)
            .map_err(io)?
            .ok_or("MissingCacheReferenceSettings")?;
        let doc = crate::settings_model::decode(&bytes)
            .map_err(|_| "CacheReferenceSettingsUnreadable")?;
        refs.extend(
            doc.selected_sounds()
                .into_iter()
                .filter(|id| id_valid(id))
                .cloned(),
        );
    }
    Ok(refs)
}
#[derive(Default, Debug)]
pub(super) struct Report {
    pub files_deleted: usize,
    pub reset: bool,
}
pub(super) fn cleanup_at(root: &Path, now: SystemTime) -> Result<Report, String> {
    // Caller holds GENERATION. Settings commits never take GENERATION.
    let _refs = REFERENCES
        .try_lock()
        .map_err(|_| "CacheReferenceSaveInProgress")?;
    let dir = root.join("voicevox");
    paths::safe(&dir).map_err(io)?;
    if !dir.exists() {
        return Ok(Report::default());
    }
    let _parents = paths::directory_chain(&dir).map_err(io)?;
    let refs = references(root)?; // Any failure suppresses every deletion, including .tmp.
    RECENT
        .lock()
        .map_err(|_| "VoicevoxCacheBusy")?
        .retain(|_, time| time.elapsed() < Duration::from_secs(300));
    let now = seconds(now)?;
    let (mut state, reset) = read_state(&dir, now)?;
    if now < state.observed {
        return Err("CacheClockMovedBackwards".into());
    }
    let mut groups: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    let mut temps = vec![];
    let mut debris = BTreeMap::new();
    for e in fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let p = e.map_err(|e| e.to_string())?.path();
        let Some(name) = p.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if let Some(id) = cache_name(name) {
            groups.entry(id).or_default().push(p);
        } else if quarantine_name(name) {
            debris.insert(name.to_string(), p);
        } else if name.strip_suffix(".tmp").is_some_and(uuid_name) {
            temps.push(p);
        }
    }
    state.unused.retain(|id, _| groups.contains_key(id));
    state.debris.retain(|name, _| debris.contains_key(name));
    for name in debris.keys() {
        state.debris.entry(name.clone()).or_insert(now);
    }
    for id in groups.keys() {
        let uses = USES.lock().map_err(|_| "VoicevoxCacheBusy")?;
        if refs.contains(id) || protected(&uses, root, id) || recent(id) {
            state.unused.remove(id);
        } else {
            state.unused.entry(id.clone()).or_insert(now);
        }
    }
    state.observed = now;
    state.checksum = state.checksum();
    let bytes = serde_json::to_vec(&state).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > STATE_LIMIT {
        return Err("CacheBookkeepingLimit".into());
    }
    publish(&dir, "gc-state.json", &bytes)?; // Persist grace periods BEFORE deleting anything.
    let mut report = Report {
        reset,
        ..Report::default()
    };
    if reset {
        return Ok(report);
    }
    for (id, files) in groups {
        let Some(since) = state.unused.get(&id) else {
            continue;
        };
        if now.saturating_sub(*since) < WEEK {
            continue;
        }
        let uses = USES.lock().map_err(|_| "VoicevoxCacheBusy")?;
        if refs.contains(&id) || protected(&uses, root, &id) || recent(&id) {
            continue;
        }
        for p in files {
            if let Ok(file) = paths::open_read(&p) {
                drop(file);
                if fs::remove_file(p).is_ok() {
                    report.files_deleted += 1;
                }
            }
        }
    }
    for p in temps {
        if let Ok(file) = paths::open_read(&p) {
            let age = file.metadata().ok().and_then(|m| m.modified().ok());
            drop(file);
            if age
                .and_then(|t| seconds(t).ok())
                .is_some_and(|t| now.saturating_sub(t) >= TEMP_AGE)
                && fs::remove_file(p).is_ok()
            {
                report.files_deleted += 1;
            }
        }
    }
    for (name, p) in debris {
        if state
            .debris
            .get(&name)
            .is_some_and(|t| now.saturating_sub(*t) >= WEEK)
        {
            if let Ok(f) = paths::open_read(&p) {
                drop(f);
                if fs::remove_file(p).is_ok() {
                    report.files_deleted += 1;
                }
            }
        }
    }
    Ok(report)
}
pub(super) fn capacity(dir: &Path, incoming: &[(String, u64)], limit: u64) -> Result<(), String> {
    let mut used = 0u64;
    for e in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let m = fs::symlink_metadata(e.path()).map_err(|e| e.to_string())?;
        if m.is_file() {
            used = used.checked_add(m.len()).ok_or("CacheSizeOverflow")?;
        }
    }
    let mut replaced = 0u64;
    let mut temporary = 0u64;
    let mut new = 0u64;
    for (name, len) in incoming {
        let p = dir.join(name);
        paths::safe(&p).map_err(io)?;
        if p.exists() {
            let f = paths::open_read(&p).map_err(io)?;
            replaced = replaced
                .checked_add(f.metadata().map_err(|e| e.to_string())?.len())
                .ok_or("CacheSizeOverflow")?;
        }
        new = new.checked_add(*len).ok_or("CacheSizeOverflow")?;
        temporary = temporary.checked_add(*len).ok_or("CacheSizeOverflow")?;
    }
    if used
        .saturating_sub(replaced)
        .checked_add(new)
        .is_none_or(|n| n > limit)
    {
        return Err("音声キャッシュの容量上限（1GB）に達しました。".into());
    }
    let wide = dir
        .to_string_lossy()
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut free = 0u64;
    if unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err("音声保存先の空き容量を確認できません。".into());
    }
    if free < temporary.saturating_add(1024 * 1024) {
        return Err("音声を安全に保存するための一時領域が不足しています。ディスクの空き容量を確保してください。".into());
    }
    Ok(())
}

#[derive(Default)]
struct Queue {
    pending: HashMap<PathBuf, Instant>,
    last: HashMap<PathBuf, Instant>,
}
fn enqueue(queue: &mut Queue, root: PathBuf, now: Instant) {
    let due = queue
        .last
        .get(&root)
        .map(|t| *t + Duration::from_secs(60))
        .unwrap_or(now)
        .max(now + Duration::from_secs(2));
    queue.pending.entry(root).or_insert(due);
}
static QUEUE: LazyLock<(Mutex<Queue>, Condvar)> =
    LazyLock::new(|| (Mutex::new(Queue::default()), Condvar::new()));
static WORKER: OnceLock<()> = OnceLock::new();
pub(super) fn request(root: PathBuf) {
    // Existing tests use disposable stores and explicitly call cleanup_at; no detached test IO.
    #[cfg(test)]
    {
        let _ = root;
        return;
    }
    #[cfg(not(test))]
    {
        WORKER.get_or_init(|| {
            if let Err(e) = std::thread::Builder::new()
                .name("cuckoo-voicevox-gc".into())
                .spawn(worker)
            {
                eprintln!("VOICEVOX cleanup worker unavailable: {e}");
            }
        });
        if let Ok(mut queue) = QUEUE.0.lock() {
            enqueue(&mut queue, root, Instant::now());
            QUEUE.1.notify_one();
        }
    }
}
fn worker() {
    loop {
        let root = {
            let Ok(mut queue) = QUEUE.0.lock() else {
                return;
            };
            loop {
                let Some((root, due)) = queue
                    .pending
                    .iter()
                    .min_by_key(|(_, t)| **t)
                    .map(|(r, t)| (r.clone(), *t))
                else {
                    queue = match QUEUE.1.wait(queue) {
                        Ok(q) => q,
                        Err(_) => return,
                    };
                    continue;
                };
                if let Some(wait) = due.checked_duration_since(Instant::now()) {
                    queue = match QUEUE.1.wait_timeout(queue, wait) {
                        Ok((q, _)) => q,
                        Err(_) => return,
                    };
                    continue;
                }
                queue.pending.remove(&root);
                queue.last.insert(root.clone(), Instant::now());
                break root;
            }
        };
        if let Ok(_generation) = GENERATION.try_lock() {
            match cleanup_at(&root, SystemTime::now()) {
                Ok(report) if report.reset => {
                    eprintln!("VOICEVOX cleanup bookkeeping reset; deletion deferred")
                }
                Err(e) => eprintln!("VOICEVOX cleanup skipped: {e}"),
                _ => {}
            }
        } else {
            request(root)
        }
    }
}
#[cfg(test)]
#[path = "voicevox_cache_tests.rs"]
mod tests;
