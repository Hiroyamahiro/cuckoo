//! D1-only sidecar catalog. No IPC path input, no playback routing, no v7 mutation.
#![allow(dead_code)] // Rust management API is intentionally not exposed by normal UI yet.
#[path = "sound_library_paths.rs"]
pub(crate) mod paths;
#[cfg(test)]
#[path = "sound_library_playback_tests.rs"]
mod playback_tests;
#[cfg(test)]
#[path = "sound_library_tests.rs"]
mod tests;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use windows_sys::Win32::Storage::FileSystem::*;
pub const MAX_FILE: u64 = 25 * 1024 * 1024;
pub const MAX_TOTAL: u64 = 1024 * 1024 * 1024;
pub const MAX_COUNT: usize = 100;
const MAX_META: u64 = 1024 * 1024;
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Error {
    pub code: String,
    pub uncertain: bool,
}
type Result<T> = std::result::Result<T, Error>;
fn err(code: &str) -> Error {
    Error {
        code: code.into(),
        uncertain: false,
    }
}
fn uncertain(code: &str) -> Error {
    Error {
        code: code.into(),
        uncertain: true,
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        err(&format!(
            "Io:{:?}:{}",
            e.kind(),
            e.raw_os_error().unwrap_or(0)
        ))
    }
}
fn sha(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
fn hash_valid(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn uuid_valid(s: &str) -> bool {
    Uuid::parse_str(s).is_ok_and(|u| {
        u.get_version_num() == 4 && u.get_variant() == uuid::Variant::RFC4122 && u.to_string() == s
    })
}
pub fn id_valid(s: &str) -> bool {
    s.strip_prefix("user:").is_some_and(uuid_valid)
}
pub fn validate_ui_name(name: &str) -> Result<()> {
    if name_valid(name) {
        Ok(())
    } else {
        Err(err("InvalidSoundName"))
    }
}
#[cfg(feature = "sound-ui-test-harness")]
pub(crate) fn verification_pick(n: u64) -> Result<Option<SelectedMp3>> {
    if n == 5 {
        return Ok(None);
    }
    let bytes = if n == 6 {
        &b"E2 invalid MP3 fixture"[..]
    } else {
        crate::builtin_audio::ENTRIES
            .get(n as usize - 1)
            .ok_or_else(|| err("InvalidPickerFixture"))?
            .bytes
    };
    SelectedMp3::verification(bytes).map(Some)
}
fn name_valid(s: &str) -> bool {
    s == s.trim()
        && !s.is_empty()
        && s.encode_utf16().count() <= 80
        && !s.chars().any(char::is_control)
}
fn quota(size: u64, frames: u64, rate: u32, count: usize, used: u64) -> Result<()> {
    if size == 0 || size > MAX_FILE {
        return Err(err("FileSizeLimit"));
    }
    if ![44100, 48000].contains(&rate)
        || frames == 0
        || frames > rate as u64 * 60
        || frames * 1000 < rate as u64
    {
        return Err(err("DurationLimit"));
    }
    if count >= MAX_COUNT {
        return Err(err("CountLimit"));
    }
    if used.checked_add(size).is_none_or(|n| n > MAX_TOTAL) {
        return Err(err("CapacityLimit"));
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Sound {
    pub id: String,
    pub name: String,
    pub sha256: String,
    pub byte_length: u64,
    pub sample_frames: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_ms: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: u32,
    pub generation: u64,
    pub sounds: Vec<Sound>,
}
/// Read-only inspection; never acquires a second management writer or repairs files.
pub(crate) fn inspect(root: &Path) -> Result<(Catalog, Option<crate::settings_model::CatalogRef>)> {
    inspect_inner(root, false)
}
pub(crate) fn inspect_pending_first(
    root: &Path,
) -> Result<(Catalog, Option<crate::settings_model::CatalogRef>)> {
    inspect_inner(root, true)
}
fn inspect_inner(
    root: &Path,
    allow_pending_first: bool,
) -> Result<(Catalog, Option<crate::settings_model::CatalogRef>)> {
    let raw = paths::read(&root.join("sounds/catalog.json"), MAX_META)?;
    match raw {
        None => {
            for name in ["backups"] {
                let p = root.join("sounds").join(name);
                paths::safe(&p)?;
                if p.exists() && fs::read_dir(&p)?.next().is_some() {
                    return Err(uncertain("MissingCatalogWithHistory"));
                }
            }
            // D1 writes the first registration journal BEFORE catalog creation.
            // Only an exact first-generation, previous=None journal proves no committed catalog.
            let transactions = root.join("sounds/transactions");
            paths::safe(&transactions)?;
            if transactions.exists() {
                for entry in fs::read_dir(&transactions)? {
                    if !allow_pending_first {
                        return Err(uncertain("MissingCatalogWithHistory"));
                    }
                    let entry = entry?;
                    let name = entry.file_name().to_string_lossy().to_string();
                    let id = name
                        .strip_suffix(".json")
                        .filter(|id| uuid_valid(id))
                        .ok_or_else(|| err("InvalidTransactionToken"))?;
                    let raw =
                        paths::read(&entry.path(), 65536)?.ok_or_else(|| err("MissingJournal"))?;
                    let j: Journal =
                        serde_json::from_slice(&raw).map_err(|_| err("InvalidJournal"))?;
                    if j.schema_version != 1
                        || j.transaction_id != id
                        || j.operation != "register"
                        || j.previous_hash.is_some()
                        || j.candidate_generation != 1
                        || !hash_valid(&j.candidate_hash)
                    {
                        return Err(uncertain("MissingCatalogWithHistory"));
                    }
                }
            }
            Ok((Catalog::default(), None))
        }
        Some(bytes) => {
            let catalog: Catalog =
                serde_json::from_slice(&bytes).map_err(|_| err("CatalogReadBlocked"))?;
            catalog.validate()?;
            let path = root.join("sounds/transactions");
            paths::safe(&path)?;
            if path.exists() {
                let mut journals = vec![];
                for entry in fs::read_dir(&path)? {
                    if journals.len() >= 4096 {
                        return Err(err("CatalogJournalLimit"));
                    }
                    let entry = entry?;
                    let name = entry.file_name().to_string_lossy().to_string();
                    let id = name
                        .strip_suffix(".json")
                        .filter(|id| uuid_valid(id))
                        .ok_or_else(|| err("InvalidTransactionToken"))?;
                    let raw =
                        paths::read(&entry.path(), 65536)?.ok_or_else(|| err("MissingJournal"))?;
                    let j: Journal =
                        serde_json::from_slice(&raw).map_err(|_| err("InvalidJournal"))?;
                    if j.schema_version != 1
                        || j.transaction_id != id
                        || !hash_valid(&j.candidate_hash)
                        || j.previous_hash.as_ref().is_some_and(|h| !hash_valid(h))
                        || j.candidate_generation == 0
                        || j.candidate_generation > crate::settings_model::MAX_COUNTER
                        || !["register", "unregister", "restore", "rename"].contains(&j.operation.as_str())
                    {
                        return Err(err("InvalidJournal"));
                    }
                    journals.push(j);
                }
                let current = sha(&bytes);
                for j in &journals {
                    if current != j.candidate_hash
                        && j.previous_hash.as_deref() != Some(&current)
                        && !(catalog.generation > j.candidate_generation
                            || journals.iter().any(|p| {
                                p.candidate_hash == current
                                    && p.previous_hash == j.previous_hash
                                    && p.candidate_generation == j.candidate_generation
                            }))
                    {
                        return Err(uncertain("UnresolvedJournal"));
                    }
                }
            }
            let reference = crate::settings_model::CatalogRef {
                schema_version: 1,
                generation: catalog.generation,
                sha256: sha(&bytes),
            };
            Ok((catalog, Some(reference)))
        }
    }
}
pub(crate) fn validate_reference(root: &Path, catalog: &Catalog, id: &str) -> Result<()> {
    hold_reference(root, catalog, id).map(|_| ())
}
/// Explicit settings-backup restoration keeps the CURRENT registry. Rebinding is
/// allowed only with an exact historical catalog and unchanged selected identities.
pub(crate) fn rebind_settings_backup(
    root: &Path,
    doc: &mut crate::settings_model::Document,
) -> Result<()> {
    let (current, reference) = inspect(root)?;
    if doc.catalog_ref == reference {
        return Ok(());
    }
    let historical = if let Some(old) = &doc.catalog_ref {
        let path = root.join("sounds/backups");
        paths::safe(&path)?;
        let mut found = None;
        if path.exists() {
            for entry in fs::read_dir(&path)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with("catalog.") || !name.ends_with(".json") {
                    continue;
                }
                let Some(bytes) = paths::read(&entry.path(), MAX_META)? else {
                    continue;
                };
                if sha(&bytes) != old.sha256 {
                    continue;
                }
                let c: Catalog =
                    serde_json::from_slice(&bytes).map_err(|_| err("InvalidCatalogBackup"))?;
                c.validate()?;
                if c.generation != old.generation || c.schema_version != old.schema_version {
                    return Err(err("CatalogBackupMismatch"));
                }
                found = Some(c);
                break;
            }
        }
        found.ok_or_else(|| err("CatalogBackupProofMissing"))?
    } else {
        Catalog::default()
    };
    for id in doc.selected_sounds() {
        if id_valid(id) {
            let before = historical
                .sounds
                .iter()
                .find(|s| &s.id == id)
                .ok_or_else(|| err("InvalidBackupReference"))?;
            let after = current
                .sounds
                .iter()
                .find(|s| &s.id == id)
                .ok_or_else(|| err("UnknownId"))?;
            if before.sha256 != after.sha256
                || before.byte_length != after.byte_length
                || before.sample_frames != after.sample_frames
                || before.sample_rate != after.sample_rate
                || before.channels != after.channels
            {
                return Err(err("BackupSoundIdentityChanged"));
            }
        }
    }
    doc.catalog_ref = reference;
    Ok(())
}
pub(crate) fn hold_reference(root: &Path, catalog: &Catalog, id: &str) -> Result<Option<File>> {
    if crate::voicevox::id_valid(id) {
        return crate::voicevox::hold(root,id).map(Some).map_err(|e|err(&e));
    }
    if crate::builtin_audio::find(id).is_some() {
        return Ok(None);
    }
    if !id_valid(id) {
        return Err(err("InvalidSoundId"));
    }
    let sound = catalog
        .sounds
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| err("UnknownId"))?;
    let path = root
        .join("sounds/blobs")
        .join(format!("{}.mp3", sound.sha256));
    let mut file = paths::open_read(&path)?;
    let mut bytes = vec![];
    (&mut file).take(MAX_FILE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != sound.byte_length || sha(&bytes) != sound.sha256 {
        return Err(err("BlobHashMismatch"));
    }
    let pcm = crate::mp3_decode::decode(&bytes, || false).map_err(|_| err("DecodeFailed"))?;
    if pcm.rate != sound.sample_rate
        || pcm.channels != sound.channels
        || pcm.samples.len() as u64 / pcm.channels as u64 != sound.sample_frames
    {
        return Err(err("MetadataMismatch"));
    }
    Ok(Some(file))
}
impl Default for Catalog {
    fn default() -> Self {
        Self {
            schema_version: 1,
            generation: 0,
            sounds: vec![],
        }
    }
}
impl Catalog {
    fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || (self.generation == 0 && !self.sounds.is_empty())
            || self.generation > 9_007_199_254_740_991
            || self.sounds.len() > MAX_COUNT
        {
            return Err(err("InvalidCatalogVersionOrLimit"));
        }
        let (mut ids, mut hashes) = (HashSet::new(), HashSet::new());
        for s in &self.sounds {
            if !id_valid(&s.id)
                || !hash_valid(&s.sha256)
                || !name_valid(&s.name)
                || ![1, 2].contains(&s.channels)
                || !ids.insert(&s.id)
                || !hashes.insert(&s.sha256)
            {
                return Err(err("InvalidCatalogEntry"));
            }
            quota(s.byte_length, s.sample_frames, s.sample_rate, 0, 0)?;
            if s.duration_ms != (s.sample_frames * 1000).div_ceil(s.sample_rate as u64) {
                return Err(err("InvalidDurationMetadata"));
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    transaction_id: String,
    operation: String,
    previous_hash: Option<String>,
    candidate_hash: String,
    candidate_generation: u64,
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct Recovery {
    pub committed: usize,
    pub not_committed: usize,
    pub superseded: usize,
    pub retained_staging: usize,
    pub retained_orphan_blobs: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    AfterValidation,
    AfterCopy,
    AfterJournal,
    AfterBlob,
    Backup,
    Temp,
    BeforeReplace,
    AfterReplace,
    FinalVerify,
}
/// All paths are fixed/derived; an arbitrary storage root is never a public API.
pub struct Store {
    root: PathBuf,
    deferred: bool,
    pending: Option<Catalog>,
    catalog: Catalog,
    expected: Option<String>,
    blocked: bool,
    pins: Arc<Mutex<HashMap<String, usize>>>,
    _dirs: Vec<File>,
    _lock: File,
    pub recovery: Recovery,
    #[cfg(test)]
    pub fault: Option<Stage>,
    #[cfg(test)]
    pub crash: Option<Stage>,
}
impl Store {
    fn prototype() -> Result<Self> {
        #[cfg(feature = "settings-test-harness")]
        let root = crate::settings_disk::verification_root().join("sounds");
        #[cfg(not(feature = "settings-test-harness"))]
        let root = crate::settings_disk::production_path()
            .map_err(|e| err(&e.code))?
            .parent()
            .ok_or_else(|| err("MissingPrototypeRoot"))?
            .join("sounds");
        Self::open(root)
    }
    fn open(root: PathBuf) -> Result<Self> {
        paths::lexical(&root)?;
        paths::safe(&root)?;
        let mut dirs = paths::directory_chain(&root)?;
        for name in ["blobs", "staging", "transactions", "backups"] {
            let p = root.join(name);
            paths::safe(&p)?;
            fs::create_dir_all(&p)?;
            dirs.push(paths::directory(&p)?);
        }
        let lock_path = root.join("library.lock");
        paths::safe(&lock_path)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(lock_path)?;
        paths::regular(&lock)?;
        let mut store = Self {
            root,
            deferred: false,
            pending: None,
            catalog: Catalog::default(),
            expected: None,
            blocked: false,
            pins: Arc::new(Mutex::new(HashMap::new())),
            _dirs: dirs,
            _lock: lock,
            recovery: Recovery::default(),
            #[cfg(test)]
            fault: None,
            #[cfg(test)]
            crash: None,
        };
        store.reconcile()?;
        Ok(store)
    }
    fn checkpoint(&self, stage: Stage) -> Result<()> {
        #[cfg(test)]
        {
            if self.crash == Some(stage) {
                std::process::exit(73)
            }
            if self.fault == Some(stage) {
                return Err(
                    if matches!(stage, Stage::AfterReplace | Stage::FinalVerify) {
                        uncertain("InjectedUncertain")
                    } else {
                        err("InjectedFailure")
                    },
                );
            }
        }
        let _ = stage;
        Ok(())
    }
    fn ready(&self, generation: u64) -> Result<()> {
        if self.blocked {
            return Err(uncertain("RecoveryRequired"));
        }
        if self.catalog.generation != generation {
            return Err(err("StaleGeneration"));
        }
        if self.current_hash()? != self.expected {
            return Err(uncertain("ExternalCatalogChange"));
        }
        Ok(())
    }
    fn current_hash(&self) -> Result<Option<String>> {
        Ok(paths::read(&self.root.join("catalog.json"), MAX_META)?.map(|b| sha(&b)))
    }
    fn blob(&self, hash: &str) -> Result<PathBuf> {
        if !hash_valid(hash) {
            return Err(err("InvalidHash"));
        }
        Ok(self.root.join("blobs").join(format!("{hash}.mp3")))
    }
    fn verify_sound(&self, s: &Sound) -> Result<()> {
        let b = paths::read(&self.blob(&s.sha256)?, MAX_FILE)?.ok_or_else(|| err("MissingBlob"))?;
        if b.len() as u64 != s.byte_length || sha(&b) != s.sha256 {
            return Err(err("BlobHashMismatch"));
        }
        let pcm = crate::mp3_decode::decode(&b, || false).map_err(|e| err(&e))?;
        if pcm.rate != s.sample_rate
            || pcm.channels != s.channels
            || pcm.samples.len() as u64 / pcm.channels as u64 != s.sample_frames
            || pcm.samples.iter().any(|x| x.abs() >= 1.)
        {
            return Err(err("BlobMetadataMismatch"));
        }
        Ok(())
    }
    fn verify_catalog(&self, c: &Catalog) -> Result<()> {
        c.validate()?;
        for s in &c.sounds {
            self.verify_sound(s)?;
        }
        Ok(())
    }
    pub fn snapshot(&self) -> Result<Catalog> {
        self.ready(self.catalog.generation)?;
        Ok(self.catalog.clone())
    }
    pub fn reconcile(&mut self) -> Result<Recovery> {
        self.blocked = true;
        let raw = paths::read(&self.root.join("catalog.json"), MAX_META)?;
        let catalog: Catalog = match &raw {
            Some(b) => serde_json::from_slice(b).map_err(|_| err("CatalogReadBlocked"))?,
            None => Catalog::default(),
        };
        catalog.validate()?;
        let current = raw.as_ref().map(|b| sha(b));
        let mut recovery = Recovery::default();
        if raw.is_none() && fs::read_dir(self.root.join("backups"))?.next().is_some() {
            return Err(uncertain("MissingCatalogWithBackups"));
        }
        let mut journals = Vec::new();
        for entry in fs::read_dir(self.root.join("transactions"))? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let token = name
                .strip_suffix(".json")
                .ok_or_else(|| err("UnknownTransactionFile"))?;
            if !uuid_valid(token) {
                return Err(err("InvalidTransactionToken"));
            }
            let b = paths::read(&entry.path(), 65536)?.ok_or_else(|| err("MissingJournal"))?;
            let j: Journal = serde_json::from_slice(&b).map_err(|_| err("InvalidJournal"))?;
            if j.schema_version != 1
                || j.transaction_id != token
                || !hash_valid(&j.candidate_hash)
                || j.candidate_generation == 0
                || j.candidate_generation > 9_007_199_254_740_991
                || j.previous_hash.as_ref().is_some_and(|h| !hash_valid(h))
                || !["register", "unregister", "restore", "rename"].contains(&j.operation.as_str())
            {
                return Err(err("InvalidJournal"));
            }
            journals.push(j);
        }
        for j in &journals {
            if current.as_deref() == Some(&j.candidate_hash) {
                recovery.committed += 1;
            } else if current == j.previous_hash {
                recovery.not_committed += 1;
            } else if raw.is_some()
                && (catalog.generation > j.candidate_generation
                    || journals.iter().any(|proof| {
                        Some(proof.candidate_hash.as_str()) == current.as_deref()
                            && proof.previous_hash == j.previous_hash
                            && proof.candidate_generation == j.candidate_generation
                    }))
            {
                recovery.superseded += 1;
            } else {
                return Err(uncertain("UnresolvedJournal"));
            }
        }
        // Nothing is deleted/promoted on restart. Unknown paths block rather than being touched.
        for entry in fs::read_dir(self.root.join("staging"))? {
            let e = entry?;
            let n = e.file_name().to_string_lossy().into_owned();
            if !n.strip_suffix(".part").is_some_and(uuid_valid) {
                return Err(err("InvalidStagingToken"));
            }
            let _ = paths::read(&e.path(), MAX_FILE)?;
            recovery.retained_staging += 1;
        }
        for entry in fs::read_dir(self.root.join("blobs"))? {
            let e = entry?;
            let n = e.file_name().to_string_lossy().into_owned();
            let h = n
                .strip_suffix(".mp3")
                .filter(|h| hash_valid(h))
                .ok_or_else(|| err("InvalidBlobToken"))?;
            let f = paths::open_read(&e.path())?;
            drop(f);
            if !catalog.sounds.iter().any(|s| s.sha256 == h) {
                recovery.retained_orphan_blobs += 1;
            }
        }
        self.catalog = catalog;
        self.expected = current;
        self.recovery = recovery.clone();
        self.blocked = false;
        Ok(recovery)
    }
    fn used(&self) -> Result<u64> {
        let mut used = 0u64;
        for sub in ["blobs", "staging"] {
            for e in fs::read_dir(self.root.join(sub))? {
                let f = paths::open_read(&e?.path())?;
                used = used
                    .checked_add(f.metadata()?.len())
                    .ok_or_else(|| err("CapacityOverflow"))?;
            }
        }
        Ok(used)
    }
    fn journal(&mut self, c: &Catalog, operation: &str) -> Result<()> {
        let candidate = serde_json::to_vec(c).map_err(|_| err("Serialize"))?;
        let j = Journal {
            schema_version: 1,
            transaction_id: Uuid::new_v4().to_string(),
            operation: operation.into(),
            previous_hash: self.expected.clone(),
            candidate_hash: sha(&candidate),
            candidate_generation: c.generation,
        };
        let bytes = serde_json::to_vec(&j).map_err(|_| err("Serialize"))?;
        let saved = paths::write_new(
            &self
                .root
                .join("transactions")
                .join(format!("{}.json", j.transaction_id)),
            &bytes,
        );
        if let Err(e) = saved {
            self.blocked = true;
            return Err(e);
        }
        Ok(())
    }
    fn commit(&mut self, c: Catalog) -> Result<()> {
        if self.deferred {
            self.verify_catalog(&c)?;
            self.pending = Some(c);
            return Ok(());
        }
        if let Some(root) = self.root.parent() {
            if let Some(raw) = paths::read(&root.join("settings.json"), MAX_META)? {
                let doc =
                    crate::settings_model::decode(&raw).map_err(|_| err("SettingsReadBlocked"))?;
                if doc.schema_version == 8 {
                    return Err(err("SettingsCoordinatorRequired"));
                }
            }
        }
        self.commit_catalog(c)
    }
    pub(crate) fn commit_catalog(&mut self, c: Catalog) -> Result<()> {
        self.ready(
            c.generation
                .checked_sub(1)
                .ok_or_else(|| err("GenerationOverflow"))?,
        )?;
        self.verify_catalog(&c)?;
        let bytes = serde_json::to_vec(&c).map_err(|_| err("Serialize"))?;
        let hash = sha(&bytes);
        if let Some(old) = paths::read(&self.root.join("catalog.json"), MAX_META)? {
            if Some(sha(&old)) != self.expected {
                return Err(uncertain("ExternalCatalogChange"));
            }
            self.checkpoint(Stage::Backup)?;
            paths::write_new(
                &self.root.join("backups").join(format!(
                    "catalog.{}.{}.json",
                    self.catalog.generation,
                    Uuid::new_v4()
                )),
                &old,
            )?;
        }
        self.checkpoint(Stage::Temp)?;
        let temp = self.root.join(format!("catalog.{}.tmp", Uuid::new_v4()));
        paths::write_new(&temp, &bytes)?;
        self.checkpoint(Stage::BeforeReplace)?;
        if self.current_hash()? != self.expected {
            return Err(uncertain("ExternalCatalogChange"));
        }
        self.blocked = true; // Set BEFORE the OS call; no flag-only timeout unlock.
        let rollback = self.root.join("backups").join(format!(
            "catalog.{}.{}.json",
            self.catalog.generation,
            Uuid::new_v4()
        ));
        let replaced = unsafe {
            if self.expected.is_some() {
                ReplaceFileW(
                    paths::wide(&self.root.join("catalog.json")).as_ptr(),
                    paths::wide(&temp).as_ptr(),
                    paths::wide(&rollback).as_ptr(),
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                )
            } else {
                MoveFileExW(
                    paths::wide(&temp).as_ptr(),
                    paths::wide(&self.root.join("catalog.json")).as_ptr(),
                    MOVEFILE_WRITE_THROUGH,
                )
            }
        };
        if replaced == 0 {
            let e = std::io::Error::last_os_error();
            if self.current_hash().ok() == Some(self.expected.clone()) {
                self.blocked = false;
                return Err(e.into());
            }
            return Err(uncertain("CatalogReplaceUncertain"));
        }
        self.checkpoint(Stage::AfterReplace)
            .map_err(|_| uncertain("CommitUncertain"))?;
        self.checkpoint(Stage::FinalVerify)
            .map_err(|_| uncertain("CommitUncertain"))?;
        if self
            .current_hash()
            .map_err(|_| uncertain("FinalReadUncertain"))?
            .as_deref()
            != Some(&hash)
        {
            return Err(uncertain("FinalHashUncertain"));
        }
        self.verify_catalog(&c)
            .map_err(|_| uncertain("FinalReferencesUncertain"))?;
        self.catalog = c;
        self.expected = Some(hash);
        self.blocked = false;
        Ok(())
    }
    fn next_catalog(&self) -> Result<Catalog> {
        let mut c = self.catalog.clone();
        c.generation = c
            .generation
            .checked_add(1)
            .filter(|n| *n <= 9_007_199_254_740_991)
            .ok_or_else(|| err("GenerationOverflow"))?;
        Ok(c)
    }
    pub fn register(
        &mut self,
        mut selection: paths::SelectedMp3,
        name: &str,
        generation: u64,
    ) -> Result<Sound> {
        self.ready(generation)?;
        let name = name.trim();
        if selection
            .path
            .to_string_lossy()
            .replace('/', "\\")
            .to_lowercase()
            .starts_with(
                &(self
                    .root
                    .to_string_lossy()
                    .replace('/', "\\")
                    .to_lowercase()
                    + "\\"),
            )
        {
            return Err(err("ManagedSourceRejected"));
        }
        if !name_valid(name) {
            return Err(err("InvalidName"));
        }
        let mut bytes = Vec::new();
        (&mut selection.file)
            .take(MAX_FILE + 1)
            .read_to_end(&mut bytes)?;
        if bytes.is_empty() || bytes.len() as u64 > MAX_FILE {
            return Err(err("FileSizeLimit"));
        }
        let start = Instant::now();
        let pcm = crate::mp3_decode::decode(&bytes, || start.elapsed() > Duration::from_secs(30))
            .map_err(|e| err(&format!("Decode:{e}")))?;
        if pcm.samples.iter().any(|x| x.abs() >= 1.) {
            return Err(err("PeakOutOfRange"));
        }
        let frames = pcm.samples.len() as u64 / pcm.channels as u64;
        let hash = sha(&bytes);
        if let Some(old) = self.catalog.sounds.iter().find(|s| s.sha256 == hash) {
            self.verify_sound(old)?;
            return Ok(old.clone());
        }
        quota(
            bytes.len() as u64,
            frames,
            pcm.rate,
            self.catalog.sounds.len(),
            self.used()?,
        )?;
        self.checkpoint(Stage::AfterValidation)?;
        let s = Sound {
            id: format!("user:{}", Uuid::new_v4()),
            name: name.into(),
            sha256: hash.clone(),
            byte_length: bytes.len() as u64,
            sample_frames: frames,
            sample_rate: pcm.rate,
            channels: pcm.channels,
            duration_ms: (frames * 1000).div_ceil(pcm.rate as u64),
        };
        drop(pcm);
        let staging = self
            .root
            .join("staging")
            .join(format!("{}.part", Uuid::new_v4()));
        paths::write_new(&staging, &bytes)?;
        if paths::read(&staging, MAX_FILE)?.as_deref() != Some(&bytes) {
            return Err(err("StagingHashMismatch"));
        }
        self.checkpoint(Stage::AfterCopy)?;
        let mut candidate = self.next_catalog()?;
        candidate.sounds.push(s.clone());
        candidate.validate()?;
        self.journal(&candidate, "register")?;
        self.checkpoint(Stage::AfterJournal)?;
        let blob = self.blob(&hash)?;
        if let Some(old) = paths::read(&blob, MAX_FILE)? {
            if sha(&old) != hash {
                return Err(err("ExistingBlobMismatch"));
            }
        } else {
            if unsafe {
                MoveFileExW(
                    paths::wide(&staging).as_ptr(),
                    paths::wide(&blob).as_ptr(),
                    MOVEFILE_WRITE_THROUGH,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        self.verify_sound(&s)?;
        self.checkpoint(Stage::AfterBlob)?;
        self.commit(candidate)?;
        Ok(s)
    }
    pub fn rename(&mut self, id: &str, name: &str, generation: u64) -> Result<()> {
        self.ready(generation)?;
        validate_ui_name(name)?;
        if !id_valid(id) { return Err(err("InvalidId")); }
        let existing = self.catalog.sounds.iter().find(|s| s.id == id)
            .ok_or_else(|| err("UnknownId"))?;
        if existing.name == name { return Ok(()); }
        let mut candidate = self.next_catalog()?;
        candidate.sounds.iter_mut().find(|s| s.id == id).unwrap().name = name.into();
        self.journal(&candidate, "rename")?;
        self.commit(candidate)
    }
    pub fn unregister(&mut self, id: &str, generation: u64) -> Result<()> {
        self.ready(generation)?;
        if !id_valid(id) {
            return Err(err("InvalidId"));
        }
        let s = self
            .catalog
            .sounds
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| err("UnknownId"))?;
        if self
            .pins
            .lock()
            .map_err(|_| err("PinLockPoisoned"))?
            .get(&s.sha256)
            .copied()
            .unwrap_or(0)
            > 0
        {
            return Err(err("SoundPinned"));
        }
        let mut c = self.next_catalog()?;
        c.sounds.retain(|s| s.id != id);
        self.journal(&c, "unregister")?;
        self.commit(c)
    }
    pub fn pin(&self, id: &str) -> Result<Pin> {
        self.ready(self.catalog.generation)?;
        let s = self
            .catalog
            .sounds
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| err("UnknownId"))?;
        *self
            .pins
            .lock()
            .map_err(|_| err("PinLockPoisoned"))?
            .entry(s.sha256.clone())
            .or_default() += 1;
        let mut pin = Pin {
            hash: s.sha256.clone(),
            sound: s.clone(),
            pins: self.pins.clone(),
            file: None,
        };
        // The local guard already owns the pin if CreateFile blocks or fails.
        pin.file = Some(paths::open_read(&self.blob(&s.sha256)?)?);
        Ok(pin)
    }
    pub fn restore(&mut self, token: &str, generation: u64) -> Result<()> {
        self.ready(generation)?;
        let parts: Vec<_> = token.split('.').collect();
        if parts.len() != 4
            || parts[0] != "catalog"
            || parts[1].parse::<u64>().is_err()
            || !uuid_valid(parts[2])
            || parts[3] != "json"
        {
            return Err(err("InvalidBackupToken"));
        }
        let b = paths::read(&self.root.join("backups").join(token), MAX_META)?
            .ok_or_else(|| err("MissingBackup"))?;
        let mut c: Catalog = serde_json::from_slice(&b).map_err(|_| err("InvalidBackup"))?;
        self.verify_catalog(&c)?;
        for old in &self.catalog.sounds {
            if self
                .pins
                .lock()
                .map_err(|_| err("PinLockPoisoned"))?
                .get(&old.sha256)
                .copied()
                .unwrap_or(0)
                > 0
                && !c
                    .sounds
                    .iter()
                    .any(|s| s.id == old.id && s.sha256 == old.sha256)
            {
                return Err(err("SoundPinned"));
            }
        }
        c.generation = self.next_catalog()?.generation;
        self.journal(&c, "restore")?;
        self.commit(c)
    }
}
pub struct Pin {
    hash: String,
    sound: Sound,
    pins: Arc<Mutex<HashMap<String, usize>>>,
    file: Option<File>,
}
impl Pin {
    pub fn decode(
        &mut self,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<crate::mp3_decode::Pcm> {
        if cancelled() {
            return Err(err("PreparationCancelled"));
        }
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| err("MissingPinnedHandle"))?;
        let mut bytes = vec![];
        file.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 != self.sound.byte_length || sha(&bytes) != self.hash {
            return Err(err("BlobHashMismatch"));
        }
        let pcm = crate::mp3_decode::decode(&bytes, &mut cancelled)
            .map_err(|e| err(&format!("DecodeError:{e}")))?;
        if pcm.rate != self.sound.sample_rate
            || pcm.channels != self.sound.channels
            || pcm.samples.len() as u64 / pcm.channels as u64 != self.sound.sample_frames
            || pcm.samples.iter().any(|x| x.abs() >= 1.)
        {
            return Err(err("BlobMetadataMismatch"));
        }
        Ok(pcm)
    }
}
impl Drop for Pin {
    fn drop(&mut self) {
        // A blocked OS close must not be advertised as an already released pin.
        drop(self.file.take());
        if let Ok(mut p) = self.pins.lock() {
            if let Some(n) = p.get_mut(&self.hash) {
                *n -= 1;
                if *n == 0 {
                    p.remove(&self.hash);
                }
            }
        }
    }
}
/// Opaque one-shot native-picker selection; Rust API only, no arbitrary-path IPC.
#[allow(unused_imports)]
pub use paths::SelectedMp3;
#[derive(Clone, Default)]
pub struct Service {
    store: Arc<Mutex<Option<Store>>>,
    busy: Arc<AtomicBool>,
    settings: Arc<Mutex<Option<std::sync::Weak<crate::settings::MemorySettings>>>>,
}
impl Service {
    pub fn ui_catalog(&self) -> Result<serde_json::Value> {
        let (root, catalog, reference, pins) = {
            let mut slot = self
                .store
                .lock()
                .map_err(|_| uncertain("LibraryPoisoned"))?;
            if slot.is_none() {
                *slot = Some(Store::prototype()?)
            }
            let store = slot.as_ref().unwrap();
            let catalog = store.snapshot()?;
            let reference = store
                .expected
                .as_ref()
                .map(|h| crate::settings_model::CatalogRef {
                    schema_version: 1,
                    generation: catalog.generation,
                    sha256: h.clone(),
                });
            let pins = store
                .pins
                .lock()
                .map_err(|_| err("PinLockPoisoned"))?
                .clone();
            (
                store
                    .root
                    .parent()
                    .ok_or_else(|| err("InvalidRoot"))?
                    .to_path_buf(),
                catalog,
                reference,
                pins,
            )
        };
        let mut sources = vec![];
        for (id, name) in [
            ("builtin:cuckoo-clock", "鳩時計"),
            ("builtin:church-bell", "教会ベル"),
            ("builtin:pendulum-clock", "振り子時計"),
            ("builtin:school-bell", "学校のチャイム"),
        ] {
            sources.push(serde_json::json!({"id":id,"name":name,"kind":"builtin","available":true,"pinned":false}));
        }
        for sound in &catalog.sounds {
            let error = validate_reference(&root, &catalog, &sound.id).err();
            sources.push(serde_json::json!({"id":sound.id,"name":sound.name,"kind":"custom","durationMs":sound.duration_ms,"available":error.is_none(),"errorCode":error.map(|e|e.code),"pinned":pins.get(&sound.sha256).copied().unwrap_or(0)>0}));
        }
        Ok(serde_json::json!({"catalogRef":reference,"sources":sources}))
    }
    #[cfg(test)]
    pub(crate) fn test_selection(path: PathBuf) -> SelectedMp3 {
        SelectedMp3::test(path).unwrap()
    }
    #[cfg(test)]
    pub(crate) fn test_open(root: &Path) -> Self {
        Self {
            store: Arc::new(Mutex::new(Some(Store::open(root.join("sounds")).unwrap()))),
            busy: Arc::default(),
            settings: Arc::default(),
        }
    }
    pub fn attach_settings(&self, settings: &Arc<crate::settings::MemorySettings>) {
        *self.settings.lock().unwrap() = Some(Arc::downgrade(settings));
    }

    #[cfg(feature = "custom-playback-test-harness")]
    pub fn verification(
        &self,
        mode: String,
        id: Option<String>,
    ) -> Result<mpsc::Receiver<Result<serde_json::Value>>> {
        self.execute(move |store| match mode.as_str() {
            "seed-one" => {
                let entry =
                    crate::builtin_audio::find(id.as_deref().ok_or_else(|| err("MissingId"))?)
                        .ok_or_else(|| err("UnknownBuiltin"))?;
                let selection = SelectedMp3::verification(entry.bytes)?;
                Ok(serde_json::json!(store.register(
                    selection,
                    &format!("E1 {}", entry.id),
                    store.catalog.generation
                )?))
            }
            "seed" => {
                let mut sounds = vec![];
                for entry in &crate::builtin_audio::ENTRIES {
                    if store.deferred {
                        return Err(err("SeedRequiresIndividualOperation"));
                    }
                    let selection = SelectedMp3::verification(entry.bytes)?;
                    sounds.push(store.register(
                        selection,
                        &format!("D2 {}", entry.id),
                        store.catalog.generation,
                    )?);
                }
                Ok(serde_json::json!(sounds))
            }
            "catalog" => Ok(serde_json::json!(store.snapshot()?)),
            "unregister" => {
                store.unregister(
                    id.as_deref().ok_or_else(|| err("MissingId"))?,
                    store.catalog.generation,
                )?;
                Ok(serde_json::json!(true))
            }
            _ => Err(err("InvalidLibraryExperiment")),
        })
    }
    #[cfg(test)]
    pub(crate) fn test_registered(bytes: &[u8]) -> (Self, Sound, PathBuf) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("test-runtime/settings")
            .join(Uuid::new_v4().to_string());
        fs::create_dir_all(&root).unwrap();
        let source = root.join("selected.mp3");
        fs::write(&source, bytes).unwrap();
        let mut store = Store::open(root.join("sounds")).unwrap();
        let sound = store
            .register(paths::SelectedMp3::test(source).unwrap(), "D2 custom", 0)
            .unwrap();
        let blob = store.blob(&sound.sha256).unwrap();
        (
            Self {
                store: Arc::new(Mutex::new(Some(store))),
                busy: Arc::new(AtomicBool::new(false)),
                settings: Arc::default(),
            },
            sound,
            blob,
        )
    }
    /// Blocking I/O is called ONLY by the tracked audio worker. No detached prep job.
    pub fn pin_for_playback(&self, id: &str) -> Result<Pin> {
        if !id_valid(id) {
            return Err(err("InvalidId"));
        }
        let mut slot = self
            .store
            .lock()
            .map_err(|_| uncertain("LibraryPoisoned"))?;
        if slot.is_none() {
            *slot = Some(Store::prototype()?);
        }
        if let Some(settings) = self
            .settings
            .lock()
            .map_err(|_| err("SettingsBridgePoisoned"))?
            .as_ref()
            .and_then(|w| w.upgrade())
        {
            let reference = settings.catalog_reference().map_err(|e| err(&e))?;
            let store = slot.as_ref().unwrap();
            if reference.as_ref().map(|r| (&r.sha256, r.generation))
                != store
                    .expected
                    .as_ref()
                    .map(|h| (h, store.catalog.generation))
            {
                return Err(uncertain("CatalogGenerationMismatch"));
            }
        }
        slot.as_mut().unwrap().pin(id)
    }
    pub fn pin_count(&self) -> Result<usize> {
        let slot = self.store.try_lock().map_err(|_| err("LibraryBusy"))?;
        match slot.as_ref() {
            None => Ok(0),
            Some(s) => Ok(s
                .pins
                .lock()
                .map_err(|_| err("PinLockPoisoned"))?
                .values()
                .sum()),
        }
    }
    /// One management job at a time, never on the scheduler or WebView thread.
    pub fn execute<T: Send + 'static>(
        &self,
        job: impl FnOnce(&mut Store) -> Result<T> + Send + 'static,
    ) -> Result<mpsc::Receiver<Result<T>>> {
        self.execute_at(None, job)
    }
    pub fn execute_at<T: Send + 'static>(
        &self,
        expected: Option<(&str, u64)>,
        job: impl FnOnce(&mut Store) -> Result<T> + Send + 'static,
    ) -> Result<mpsc::Receiver<Result<T>>> {
        if self
            .busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(err("LibraryBusy"));
        }
        let (tx, rx) = mpsc::sync_channel(1);
        let service = self.clone();
        let attached = self
            .settings
            .lock()
            .map_err(|_| err("SettingsBridgePoisoned"))?
            .as_ref()
            .and_then(|w| w.upgrade());
        if let Some(settings) = attached {
            let output = Arc::new(Mutex::new(None));
            let captured = output.clone();
            let library = self.clone();
            let queued = settings.storage_operation_at(expected, move |disk, doc| {
                let mut slot =
                    library
                        .store
                        .lock()
                        .map_err(|_| crate::settings_disk::DiskError {
                            code: "LibraryPoisoned".into(),
                            uncertain: true,
                        })?;
                if slot.is_none() {
                    *slot = Some(Store::prototype().map_err(crate::settings_disk::library_error)?);
                }
                let catalog = slot.as_mut().unwrap();
                catalog.deferred = true;
                catalog.pending = None;
                let result = job(catalog);
                catalog.deferred = false;
                match result {
                    Ok(value) => {
                        let next = if let Some(candidate) = catalog.pending.take() {
                            match disk.commit_catalog_operation(doc, catalog, candidate) {
                                Ok(d) => d,
                                Err(e) => {
                                    if e.uncertain {
                                        catalog.blocked = true;
                                    }
                                    return Err(e);
                                }
                            }
                        } else {
                            doc.clone()
                        };
                        *captured.lock().unwrap() = Some(value);
                        Ok(next)
                    }
                    Err(e) => {
                        catalog.pending = None;
                        Err(crate::settings_disk::library_error(e))
                    }
                }
            });
            match queued {
                Ok(done) => {
                    let reset = self.busy.clone();
                    thread::spawn(move || {
                        let result = match done.recv() {
                            Ok(Ok(())) => output
                                .lock()
                                .unwrap()
                                .take()
                                .ok_or_else(|| uncertain("MissingOperationResult")),
                            Ok(Err(e)) => Err(Error {
                                code: e.code,
                                uncertain: e.uncertain,
                            }),
                            Err(_) => Err(uncertain("SettingsWorkerGone")),
                        };
                        reset.store(false, Ordering::SeqCst);
                        let _ = tx.send(result);
                    });
                    return Ok(rx);
                }
                Err(e) => {
                    self.busy.store(false, Ordering::SeqCst);
                    return Err(err(&e));
                }
            }
        }
        let spawned = thread::Builder::new()
            .name("cuckoo-sound-library".into())
            .spawn(move || {
                struct Reset(Arc<AtomicBool>);
                impl Drop for Reset {
                    fn drop(&mut self) {
                        self.0.store(false, Ordering::SeqCst)
                    }
                }
                let _reset = Reset(service.busy.clone());
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut slot = service
                        .store
                        .lock()
                        .map_err(|_| uncertain("LibraryPoisoned"))?;
                    if slot.is_none() {
                        *slot = Some(Store::prototype()?)
                    }
                    job(slot.as_mut().unwrap())
                }))
                .unwrap_or_else(|_| Err(uncertain("LibraryWorkerPanic")));
                let _ = tx.send(result);
            });
        if let Err(e) = spawned {
            self.busy.store(false, Ordering::SeqCst);
            return Err(err(&format!("WorkerSpawn:{e}")));
        }
        Ok(rx)
    }
}
