//! One writer, fixed production namespace, exact-byte backup before replacement.
//! No Electron path lookup and no production path override of any kind.
#[cfg(not(feature = "settings-test-harness"))]
use crate::settings::storage_identifier;
use crate::settings_model::{self, Document, MAX_BYTES, MAX_COUNTER};
use sha2::{Digest, Sha256};
#[cfg(not(feature = "settings-test-harness"))]
use std::os::windows::ffi::OsStringExt;
use std::os::windows::{
    ffi::OsStrExt,
    fs::{MetadataExt, OpenOptionsExt},
};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;
#[cfg(feature = "sound-ui-test-harness")]
static UI_FAULT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[cfg(feature = "sound-ui-test-harness")]
pub(crate) fn arm_ui_fault(n: u64) {
    UI_FAULT.store(n, std::sync::atomic::Ordering::SeqCst);
}
#[path = "settings_pair.rs"]
mod pair;
pub(crate) fn library_error(e: crate::sound_library::Error) -> DiskError {
    DiskError {
        code: e.code,
        uncertain: e.uncertain,
    }
}
use windows_sys::Win32::Storage::FileSystem::*;
#[cfg(not(feature = "settings-test-harness"))]
use windows_sys::Win32::{
    System::Com::CoTaskMemFree,
    UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath},
};

#[derive(Debug, Clone)]
pub struct DiskError {
    pub code: String,
    pub uncertain: bool,
}
impl From<std::io::Error> for DiskError {
    fn from(e: std::io::Error) -> Self {
        Self {
            code: format!("Io:{:?}:{}", e.kind(), e.raw_os_error().unwrap_or(0)),
            uncertain: false,
        }
    }
}
fn err(code: &str) -> DiskError {
    DiskError {
        code: code.into(),
        uncertain: false,
    }
}
fn sha(b: &[u8]) -> Vec<u8> {
    Sha256::digest(b).to_vec()
}
fn wide(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain(Some(0)).collect()
}
fn safe(p: &Path) -> Result<(), DiskError> {
    for parent in p.ancestors() {
        match fs::symlink_metadata(parent) {
            Ok(m) if m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 => {
                return Err(err("ReparsePointRejected"))
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
fn read(p: &Path) -> Result<Option<Vec<u8>>, DiskError> {
    safe(p)?;
    let mut file = match OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(p)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let m = file.metadata()?;
    if !m.is_file() || m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(err("NotRegularFile"));
    }
    if m.len() > MAX_BYTES as u64 {
        return Err(err("TooLarge"));
    }
    let mut bytes = vec![];
    (&mut file)
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BYTES {
        return Err(err("TooLarge"));
    }
    Ok(Some(bytes))
}
fn new_file(p: &Path, b: &[u8]) -> Result<(), DiskError> {
    safe(p)?;
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(p)?;
    f.write_all(b)?;
    f.sync_all()?;
    drop(f);
    if read(p)?.as_deref() != Some(b) {
        return Err(err("WriteVerificationFailed"));
    }
    Ok(())
}
fn directory(p: &Path) -> Result<File, DiskError> {
    safe(p)?;
    fs::create_dir_all(p)?;
    safe(p)?;
    // Keep managed directories open without FILE_SHARE_DELETE while writing.
    Ok(OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(p)?)
}
#[cfg(not(feature = "settings-test-harness"))]
pub fn production_path() -> Result<PathBuf, DiskError> {
    let mut raw = std::ptr::null_mut();
    let hr =
        unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, 0, std::ptr::null_mut(), &mut raw) };
    if hr < 0 || raw.is_null() {
        return Err(err("LocalAppDataUnavailable"));
    }
    let root = unsafe {
        let mut len = 0;
        while *raw.add(len) != 0 {
            len += 1
        }
        let s = std::ffi::OsString::from_wide(std::slice::from_raw_parts(raw, len));
        CoTaskMemFree(raw as _);
        PathBuf::from(s)
    };
    crate::settings::reserved_settings_path(Some(&root)).map_err(err)
}

#[derive(Clone, Debug)]
pub struct Candidate {
    pub token: String,
    pub generation: u64,
}
pub enum Loaded {
    Fresh,
    Valid(Document),
    Blocked(String),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Backup,
    TempCreate,
    TempWrite,
    TempFlush,
    TempVerify,
    Replace,
    AfterReplace,
    FinalVerify,
}
pub struct Store {
    explicit_restore: bool,
    root: PathBuf,
    expected: Option<Vec<u8>>,
    pub generation: u64,
    _lock: File,
    _root_handle: File,
    _backup_handle: File,
    #[cfg(test)]
    pub fault: Option<Stage>,
    #[cfg(test)]
    pub gate: Option<(
        Stage,
        std::sync::mpsc::Sender<()>,
        std::sync::mpsc::Receiver<()>,
    )>,
    #[cfg(test)]
    crash: Option<Stage>,
}
impl Store {
    #[cfg(test)]
    pub(crate) fn inject_process_exit(&mut self, stage: Stage) {
        self.crash = Some(stage);
    }
    pub fn validate_sound(&self, id: &str) -> Result<(), DiskError> {
        let (catalog, _) = crate::sound_library::inspect(&self.root).map_err(library_error)?;
        crate::sound_library::validate_reference(&self.root, &catalog, id).map_err(library_error)
    }
    pub fn availability(&self, doc: &Document) -> serde_json::Value {
        let status = |id: &str| match self.validate_sound(id) {
            Ok(()) => serde_json::json!({"soundId":id,"state":"Available"}),
            Err(e) => serde_json::json!({"soundId":id,"state":"Unavailable","code":e.code}),
        };
        serde_json::json!({"onHour":status(&doc.audio.on_hour.sound_id),"onHalfHour":status(&doc.audio.on_half_hour.sound_id)})
    }
    pub fn prototype() -> Result<Self, DiskError> {
        #[cfg(feature = "settings-test-harness")]
        {
            return Self::open(verification_root());
        }
        #[cfg(not(feature = "settings-test-harness"))]
        {
            let path = production_path()?;
            if path
                .parent()
                .and_then(Path::file_name)
                .and_then(|s| s.to_str())
                != Some(storage_identifier())
            {
                return Err(err("NamespaceMismatch"));
            }
            Self::open(path.parent().unwrap().to_path_buf())
        }
    }
    // Only this module can supply a root in production. Tests use synthetic roots.
    fn open(root: PathBuf) -> Result<Self, DiskError> {
        if !root.is_absolute() {
            return Err(err("RelativeRootRejected"));
        }
        let rh = directory(&root)?;
        let bh = directory(&root.join("backups"))?;
        let lock_path = root.join("settings.lock");
        safe(&lock_path)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(lock_path)?;
        Ok(Self {
            explicit_restore: false,
            root,
            expected: None,
            generation: 0,
            _lock: lock,
            _root_handle: rh,
            _backup_handle: bh,
            #[cfg(test)]
            fault: None,
            #[cfg(test)]
            gate: None,
            #[cfg(test)]
            crash: None,
        })
    }
    #[cfg(test)]
    pub fn test(root: PathBuf) -> Result<Self, DiskError> {
        Self::open(root)
    }
    fn path(&self) -> PathBuf {
        self.root.join("settings.json")
    }
    fn point(&mut self, _s: Stage) -> Result<(), DiskError> {
        #[cfg(feature = "sound-ui-test-harness")]
        {
            let n = match _s {
                Stage::TempFlush => 101,
                Stage::AfterReplace => 102,
                _ => 0,
            };
            if n != 0
                && UI_FAULT
                    .compare_exchange(
                        n,
                        0,
                        std::sync::atomic::Ordering::SeqCst,
                        std::sync::atomic::Ordering::SeqCst,
                    )
                    .is_ok()
            {
                return Err(DiskError {
                    code: "InjectedUiSaveFailure".into(),
                    uncertain: n == 102,
                });
            }
        }
        #[cfg(test)]
        if self.crash == Some(_s) {
            std::process::exit(77)
        }
        #[cfg(test)]
        if self.gate.as_ref().map(|g| g.0) == Some(_s) {
            let (_, arrived, resume) = self.gate.take().unwrap();
            arrived.send(()).unwrap();
            resume
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
        }
        #[cfg(test)]
        if self.fault == Some(_s) {
            self.fault = None;
            return Err(DiskError {
                code: format!("Injected:{_s:?}"),
                uncertain: matches!(
                    _s,
                    Stage::Replace | Stage::AfterReplace | Stage::FinalVerify
                ),
            });
        }
        Ok(())
    }
    pub fn load(&mut self) -> Result<Loaded, DiskError> {
        let b = read(&self.path())?;
        self.expected = b.as_ref().map(|b| sha(b));
        if let Some(raw) = &b {
            if let Err(code) = settings_model::decode(raw) {
                if code.starts_with("Unsupported") {
                    return Ok(Loaded::Blocked(code));
                }
            }
        }
        if let Err(e) = pair::reconcile(&self.root) {
            return Ok(Loaded::Blocked(e.code));
        }
        if let Some(b) = b {
            return Ok(match settings_model::decode(&b) {
                Ok(mut d) => {
                    self.generation = d.generation;
                    if d.schema_version == 7 {
                        // Migration originals are retained independently of rolling backups.
                        new_file(
                            &self
                                .root
                                .join("backups")
                                .join(format!("settings.migration-v7-{}.json", Uuid::new_v4())),
                            &b,
                        )?;
                        d.upgrade();
                        let (_, reference) =
                            crate::sound_library::inspect(&self.root).map_err(library_error)?;
                        d.catalog_ref = reference;
                        Loaded::Valid(self.commit(d)?)
                    } else {
                        self.check_catalog(&d)?;
                        if d.audio.on_hour.sound_id == settings_model::REMOVED_WAV
                            || d.audio.on_half_hour.sound_id == settings_model::REMOVED_WAV {
                            new_file(&self.root.join("backups").join(format!("settings.migration-retired-sound-{}.json",Uuid::new_v4())), &b)?;
                            d.upgrade();
                            Loaded::Valid(self.commit(d)?)
                        } else { Loaded::Valid(d) }
                    }
                }
                Err(e) => Loaded::Blocked(e),
            });
        }
        let leftovers = fs::read_dir(&self.root)?.any(|e| {
            e.map(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("settings.write-")
            })
            .unwrap_or(true)
        });
        if leftovers || fs::read_dir(self.root.join("backups"))?.next().is_some() {
            Ok(Loaded::Blocked("RecoveryRequired".into()))
        } else {
            Ok(Loaded::Fresh)
        }
    }
    pub fn candidates(&self) -> Result<Vec<Candidate>, DiskError> {
        let mut out = vec![];
        for e in fs::read_dir(self.root.join("backups"))? {
            let e = e?;
            let token = e.file_name().to_string_lossy().to_string();
            if !valid_token(&token) {
                continue;
            }
            if let Ok(Some(bytes)) = read(&e.path()) {
                if let Ok(d) = settings_model::decode(&bytes) {
                    out.push(Candidate {
                        token,
                        generation: d.generation,
                    })
                }
            }
        }
        out.sort_by(|a, b| b.generation.cmp(&a.generation).then(a.token.cmp(&b.token)));
        Ok(out)
    }
    fn check_catalog(&self, doc: &Document) -> Result<crate::sound_library::Catalog, DiskError> {
        let (catalog, reference) =
            crate::sound_library::inspect(&self.root).map_err(library_error)?;
        if doc.catalog_ref != reference {
            return Err(DiskError {
                code: "CatalogGenerationMismatch".into(),
                uncertain: true,
            });
        }
        Ok(catalog)
    }
    pub fn commit(&mut self, mut doc: Document) -> Result<Document, DiskError> {
        if read(&self.path())?.as_ref().map(|b| sha(b)) != self.expected {
            return Err(err("ExternalChangeConflict"));
        }
        doc.upgrade();
        // Fresh defaults bind an existing independent catalog without erasing it.
        if self.expected.is_none() {
            doc.catalog_ref = crate::sound_library::inspect(&self.root)
                .map_err(library_error)?
                .1;
        }
        let catalog = self.check_catalog(&doc)?;
        let old = read(&self.path())?.and_then(|b| settings_model::decode(&b).ok());
        // Unchanged unavailable selections remain visible as unavailable; newly selected IDs must be valid.
        let mut guards = vec![];
        let mut selections = vec![
            (&doc.audio.on_hour.sound_id, old.as_ref().map(|d| &d.audio.on_hour.sound_id)),
            (&doc.audio.on_half_hour.sound_id, old.as_ref().map(|d| &d.audio.on_half_hour.sound_id)),
        ];
        selections.extend(doc.schedule.custom.iter().map(|c| (
            &c.sound_id,
            old.as_ref().and_then(|d| d.schedule.custom.iter().find(|previous| previous.id == c.id)).map(|c| &c.sound_id),
        )));
        for (id, previous) in selections {
            if previous != Some(id) {
                guards.push(crate::sound_library::hold_reference(&self.root, &catalog, id).map_err(library_error)?);
            }
        }
        let mut candidate = doc.clone();
        candidate.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| err("GenerationExhausted"))?;
        candidate.validate().map_err(|e| err(&e))?;
        let bytes = serde_json::to_vec_pretty(&candidate).map_err(|_| err("SerializeFailed"))?;
        let catalog_hash = read(&self.root.join("sounds/catalog.json"))?.map(|b| sha(&b));
        let pair = if self.explicit_restore {
            pair::begin_restore(&self.root, &bytes, catalog_hash)?
        } else {
            pair::begin(&self.root, &bytes, catalog_hash)?
        };
        let saved = self.commit_raw(doc)?;
        pair::finish(&self.root, &pair)?;
        self.check_catalog(&saved).map_err(|mut e| {
            e.uncertain = true;
            e
        })?;
        drop(guards);
        Ok(saved)
    }
    pub(crate) fn commit_catalog_operation(
        &mut self,
        doc: &Document,
        store: &mut crate::sound_library::Store,
        catalog: crate::sound_library::Catalog,
    ) -> Result<Document, DiskError> {
        let previous_catalog = if doc.catalog_ref.is_none() && catalog.generation == 1 {
            let (old, reference) =
                crate::sound_library::inspect_pending_first(&self.root).map_err(library_error)?;
            if reference.is_some() {
                return Err(err("CatalogGenerationMismatch"));
            }
            old
        } else {
            self.check_catalog(doc)?
        };
        for id in doc.selected_sounds() {
            if crate::sound_library::id_valid(id) && !catalog.sounds.iter().any(|s| &s.id == id) {
                return Err(err("SoundSelected"));
            }
            if let (Some(old), Some(new)) = (
                previous_catalog.sounds.iter().find(|s| &s.id == id),
                catalog.sounds.iter().find(|s| &s.id == id),
            ) {
                if old.sha256 != new.sha256 {
                    return Err(err("SelectedSoundIdentityChanged"));
                }
            }
        }
        let bytes = serde_json::to_vec(&catalog).map_err(|_| err("SerializeFailed"))?;
        let mut next = doc.clone();
        next.catalog_ref = Some(settings_model::CatalogRef {
            schema_version: 1,
            generation: catalog.generation,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
        });
        next.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| err("GenerationExhausted"))?;
        next.validate().map_err(|e| err(&e))?;
        let target = serde_json::to_vec_pretty(&next).map_err(|_| err("SerializeFailed"))?;
        let pair = pair::begin(&self.root, &target, Some(sha(&bytes)))?;
        store.commit_catalog(catalog).map_err(library_error)?;
        let saved = self.commit_raw(next).map_err(|mut e| {
            e.uncertain = true;
            e
        })?;
        pair::finish(&self.root, &pair)?;
        self.check_catalog(&saved).map_err(|mut e| {
            e.uncertain = true;
            e
        })?;
        Ok(saved)
    }
    fn commit_raw(&mut self, mut doc: Document) -> Result<Document, DiskError> {
        doc.validate().map_err(|s| err(&s))?;
        let old = read(&self.path())?;
        if old.as_ref().map(|b| sha(b)) != self.expected {
            return Err(err("ExternalChangeConflict"));
        }
        let generation = self
            .generation
            .checked_add(1)
            .filter(|g| *g <= MAX_COUNTER)
            .ok_or_else(|| err("GenerationExhausted"))?;
        doc.generation = generation;
        let bytes = serde_json::to_vec_pretty(&doc).map_err(|_| err("SerializeFailed"))?;
        if bytes.len() > MAX_BYTES {
            return Err(err("TooLarge"));
        }
        // Exact old bytes are durable and verified BEFORE creating the replacement.
        if let Some(old) = &old {
            self.point(Stage::Backup)?;
            let p = self.root.join("backups").join(format!(
                "settings.g{}-{}.json",
                self.generation,
                Uuid::new_v4()
            ));
            new_file(&p, old)?;
        }
        self.point(Stage::TempCreate)?;
        let tmp = self
            .root
            .join(format!("settings.write-{}.tmp", Uuid::new_v4()));
        safe(&tmp)?;
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&tmp)?;
        self.point(Stage::TempWrite)?;
        f.write_all(&bytes)?;
        self.point(Stage::TempFlush)?;
        f.sync_all()?;
        drop(f);
        self.point(Stage::TempVerify)?;
        let verified = read(&tmp)?.ok_or_else(|| err("MissingTemp"))?;
        settings_model::decode(&verified).map_err(|s| err(&s))?;
        if sha(&verified) != sha(&bytes) {
            return Err(err("TempMismatch"));
        }
        // Recheck after temp I/O; external, non-cooperating writers are unsupported.
        if read(&self.path())?.as_ref().map(|b| sha(b)) != self.expected {
            return Err(err("ExternalChangeConflict"));
        }
        self.point(Stage::Replace)?;
        let rollback = self
            .root
            .join("backups")
            .join(format!("replace-rollback.{}.json", Uuid::new_v4()));
        safe(&self.path())?;
        safe(&rollback)?;
        let success = unsafe {
            if old.is_some() {
                ReplaceFileW(
                    wide(&self.path()).as_ptr(),
                    wide(&tmp).as_ptr(),
                    wide(&rollback).as_ptr(),
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                )
            } else {
                MoveFileExW(
                    wide(&tmp).as_ptr(),
                    wide(&self.path()).as_ptr(),
                    MOVEFILE_WRITE_THROUGH,
                )
            }
        };
        if success == 0 {
            let mut e: DiskError = std::io::Error::last_os_error().into();
            e.uncertain = true;
            return Err(e);
        }
        self.point(Stage::AfterReplace)?;
        self.point(Stage::FinalVerify)?;
        let final_bytes = read(&self.path())
            .map_err(|mut e| {
                e.uncertain = true;
                e
            })?
            .ok_or_else(|| DiskError {
                code: "MissingAfterCommit".into(),
                uncertain: true,
            })?;
        if sha(&final_bytes) != sha(&bytes) || settings_model::decode(&final_bytes).is_err() {
            return Err(DiskError {
                code: "CommitUncertain".into(),
                uncertain: true,
            });
        }
        self.expected = Some(sha(&bytes));
        self.generation = generation;
        // Cleanup failure cannot retroactively make a verified commit unsaved.
        if old.is_some() {
            let _ = fs::remove_file(rollback);
        }
        self.prune();
        Ok(doc)
    }
    fn prune(&self) {
        if let Ok(c) = self.candidates() {
            let mut generations = std::collections::HashSet::new();
            for x in c.into_iter().filter(|c| c.token.starts_with("settings.g")) {
                generations.insert(x.generation);
                if generations.len() > 5 {
                    let _ = fs::remove_file(self.root.join("backups").join(x.token));
                }
            }
        }
        // Do not delete unknown, invalid, or interrupted files automatically.
    }
    pub fn restore(&mut self, token: &str) -> Result<Document, DiskError> {
        if !valid_token(token) {
            return Err(err("InvalidBackupToken"));
        }
        let b =
            read(&self.root.join("backups").join(token))?.ok_or_else(|| err("MissingBackup"))?;
        let mut d = settings_model::decode(&b).map_err(|s| err(&s))?;
        if d.schema_version == 7 {
            d.upgrade();
            d.catalog_ref = crate::sound_library::inspect(&self.root)
                .map_err(library_error)?
                .1;
        }
        d.upgrade();
        crate::sound_library::rebind_settings_backup(&self.root, &mut d).map_err(library_error)?;
        let catalog = self.check_catalog(&d)?;
        for id in d.selected_sounds() {
            crate::sound_library::validate_reference(&self.root, &catalog, id)
                .map_err(library_error)?;
        }
        // Current bytes must still match the state on which confirmation was based.
        let original = read(&self.path())?;
        if original.as_ref().map(|b| sha(b)) != self.expected {
            return Err(err("ExternalChangeConflict"));
        }
        if let Some(bytes) = &original {
            if let Err(code) = settings_model::decode(bytes) {
                if code.starts_with("Unsupported") {
                    return Err(err(&code));
                }
            }
            if settings_model::decode(bytes).is_err() {
                let q = self.root.join("quarantine");
                let _handle = directory(&q)?;
                new_file(&q.join(format!("{}.json", Uuid::new_v4())), bytes)?;
            }
        }
        let known = self
            .candidates()?
            .iter()
            .map(|c| c.generation)
            .max()
            .unwrap_or(0);
        self.generation = self.generation.max(d.generation).max(known);
        self.explicit_restore = true;
        let result = self.commit(d);
        self.explicit_restore = false;
        result
    }
}
#[cfg(feature = "settings-test-harness")]
pub fn verification_root() -> PathBuf {
    // Compile-time UUID, NOT an arbitrary root or runtime override. This feature
    // is never enabled in the distributable release build.
    let run = Uuid::parse_str(env!("CUCKOO_T3B_TEST_RUN_ID")).expect("Test build requires UUID");
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("test-runtime/harness")
        .join(run.to_string())
}
fn valid_token(t: &str) -> bool {
    !t.is_empty()
        && t.len() < 160
        && t.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
        && (t.starts_with("settings.g")
            || t.starts_with("settings.migration-v7-")
            || t.starts_with("settings.migration-retired-sound-")
            || t.starts_with("replace-rollback."))
        && t.ends_with(".json")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retired_wav_migration_preserves_original_and_other_settings() {
        let t=Temp::new();
        let mut store=Store::test(t.0.clone()).unwrap();
        let mut doc=Document::default();
        let file=store.path();drop(store);
        doc.audio.on_hour.sound_id=settings_model::REMOVED_WAV.into();
        doc.audio.on_half_hour.sound_id=settings_model::REMOVED_WAV.into();
        doc.audio.on_half_hour.repeat=2;doc.audio.volume=37;
        let raw=serde_json::to_vec(&doc).unwrap();fs::write(&file,&raw).unwrap();
        let mut store=Store::test(t.0.clone()).unwrap();
        let Loaded::Valid(migrated)=store.load().unwrap() else {panic!("Migration failed")};
        assert_eq!(migrated.audio.on_hour.sound_id,settings_model::CUCKOO);
        assert_eq!(migrated.audio.on_half_hour.sound_id,settings_model::CUCKOO);
        assert_eq!(migrated.audio.on_half_hour.repeat,2);
        assert_eq!(migrated.audio.volume,37);
        assert_eq!(migrated.schedule,doc.schedule);
        assert_eq!(migrated.presets,doc.presets);
        assert_eq!(migrated.generation,doc.generation+1);
        assert!(fs::read_dir(t.0.join("backups")).unwrap().any(|e|fs::read(e.unwrap().path()).unwrap()==raw));
        assert!(migrated.select_sound("hour",settings_model::REMOVED_WAV).is_err());
        let generation=migrated.generation;drop(store);
        let mut store=Store::test(t.0.clone()).unwrap();
        let Loaded::Valid(again)=store.load().unwrap() else {panic!()};
        assert_eq!(again.generation,generation);
    }
    #[test]
    fn retired_wav_migration_failure_does_not_replace_original() {
        let t=Temp::new();let mut store=Store::test(t.0.clone()).unwrap();
        let mut doc=Document::default();let file=store.path();drop(store);
        doc.audio.on_hour.sound_id=settings_model::REMOVED_WAV.into();
        let raw=serde_json::to_vec(&doc).unwrap();fs::write(&file,&raw).unwrap();
        let mut store=Store::test(t.0.clone()).unwrap();store.fault=Some(Stage::TempFlush);
        assert!(store.load().is_err());assert_eq!(fs::read(&file).unwrap(),raw);drop(store);
        let mut store=Store::test(t.0.clone()).unwrap();
        let Loaded::Valid(doc)=store.load().unwrap() else {panic!()};
        assert_eq!(doc.audio.on_hour.sound_id,settings_model::CUCKOO);
    }
    pub struct Temp(pub PathBuf);
    impl Temp {
        pub fn new() -> Self {
            Self(std::env::temp_dir().join(format!("cuckoo-t3b-test-{}", Uuid::new_v4())))
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn crash_fixture() {
        let Ok(root) = std::env::var("CUCKOO_T3B_CRASH_ROOT") else {
            return;
        };
        let root = PathBuf::from(root);
        assert!(root.starts_with(std::env::temp_dir()));
        assert!(root
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("cuckoo-t3b-test-"));
        let mut s = Store::test(root).unwrap();
        s.load().unwrap();
        s.crash = Some(
            if std::env::var("CUCKOO_T3B_CRASH_STAGE").unwrap() == "after" {
                Stage::AfterReplace
            } else {
                Stage::TempVerify
            },
        );
        let mut d = Document::default();
        d.audio.volume = 37;
        s.commit(d).unwrap();
        panic!("Crash point was not reached");
    }
    #[test]
    fn process_exit_before_and_after_replace_recovers_without_promoting_temp() {
        for stage in ["before", "after"] {
            let t = Temp::new();
            let mut s = Store::test(t.0.clone()).unwrap();
            s.load().unwrap();
            s.commit(Document::default()).unwrap();
            let original = read(&s.path()).unwrap().unwrap();
            drop(s);
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "settings_disk::tests::crash_fixture",
                    "--nocapture",
                ])
                .env("CUCKOO_T3B_CRASH_ROOT", &t.0)
                .env("CUCKOO_T3B_CRASH_STAGE", stage)
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(77));
            let mut s = Store::test(t.0.clone()).unwrap();
            let Loaded::Valid(d) = s.load().unwrap() else {
                panic!("Valid primary must survive")
            };
            assert_eq!(d.audio.volume, if stage == "before" { 50 } else { 37 });
            assert!(s.candidates().unwrap().iter().any(|c| read(
                &s.root.join("backups").join(&c.token)
            )
            .unwrap()
            .as_ref()
                == Some(&original)));
        }
    }
    #[test]
    fn os_sharing_denial_preserves_primary_and_retry_succeeds() {
        let t = Temp::new();
        let mut s = Store::test(t.0.clone()).unwrap();
        s.load().unwrap();
        s.commit(Document::default()).unwrap();
        let original = read(&s.path()).unwrap().unwrap();
        let held = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(s.path())
            .unwrap();
        assert!(s.commit(Document::default()).is_err());
        drop(held);
        assert_eq!(read(&s.path()).unwrap().unwrap(), original);
        s.commit(Document::default()).unwrap();
    }
    #[test]
    fn first_save_restart_backup_and_lock() {
        let t = Temp::new();
        let mut s = Store::test(t.0.clone()).unwrap();
        assert!(matches!(s.load().unwrap(), Loaded::Fresh));
        assert!(Store::test(t.0.clone()).is_err());
        let d = s.commit(Document::default()).unwrap();
        assert_eq!(d.generation, 1);
        let mut d = d;
        d.audio.volume = 37;
        assert_eq!(s.commit(d).unwrap().generation, 2);
        let c = s.candidates().unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].generation, 1);
        drop(s);
        let mut s = Store::test(t.0.clone()).unwrap();
        match s.load().unwrap() {
            Loaded::Valid(d) => assert_eq!(d.audio.volume, 37),
            _ => panic!(),
        }
    }
    #[test]
    fn all_save_failure_points_preserve_last_good() {
        for point in [
            Stage::Backup,
            Stage::TempCreate,
            Stage::TempWrite,
            Stage::TempFlush,
            Stage::TempVerify,
            Stage::Replace,
            Stage::AfterReplace,
            Stage::FinalVerify,
        ] {
            let t = Temp::new();
            let mut s = Store::test(t.0.clone()).unwrap();
            s.load().unwrap();
            s.commit(Document::default()).unwrap();
            let old = read(&s.path()).unwrap().unwrap();
            s.fault = Some(point);
            let mut d = Document::default();
            d.audio.volume = 37;
            let e = s.commit(d).unwrap_err();
            assert_eq!(
                e.uncertain,
                matches!(
                    point,
                    Stage::Replace | Stage::AfterReplace | Stage::FinalVerify
                )
            );
            let primary = read(&s.path()).unwrap().unwrap();
            settings_model::decode(&primary).unwrap();
            if !matches!(point, Stage::AfterReplace | Stage::FinalVerify) {
                assert_eq!(primary, old)
            }
            if point != Stage::Backup {
                assert!(s.candidates().unwrap().iter().any(|c| read(
                    &s.root.join("backups").join(&c.token)
                )
                .unwrap()
                .as_ref()
                    == Some(&old)));
            }
        }
    }
    #[test]
    fn corrupt_restore_is_explicit_and_preserves_original() {
        let t = Temp::new();
        let mut s = Store::test(t.0.clone()).unwrap();
        s.load().unwrap();
        s.commit(Document::default()).unwrap();
        s.commit(Document::default()).unwrap();
        drop(s);
        fs::write(t.0.join("settings.json"), b"{broken").unwrap();
        let mut s = Store::test(t.0.clone()).unwrap();
        assert!(matches!(s.load().unwrap(), Loaded::Blocked(_)));
        assert_eq!(fs::read(t.0.join("settings.json")).unwrap(), b"{broken");
        let c = s.candidates().unwrap();
        let d = s.restore(&c[0].token).unwrap();
        assert!(d.generation > c[0].generation);
        let q = fs::read_dir(t.0.join("quarantine"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(fs::read(q.path()).unwrap(), b"{broken");
        assert!(s.restore("../settings.json").is_err());
    }
    #[test]
    fn external_change_and_missing_with_backups_block() {
        let t = Temp::new();
        let mut s = Store::test(t.0.clone()).unwrap();
        s.load().unwrap();
        s.commit(Document::default()).unwrap();
        fs::write(s.path(), b"externally changed").unwrap();
        assert_eq!(
            s.commit(Document::default()).unwrap_err().code,
            "ExternalChangeConflict"
        );
        drop(s);
        fs::remove_file(t.0.join("settings.json")).unwrap();
        fs::write(t.0.join("backups/unknown.json"), b"do not replace").unwrap();
        assert!(matches!(
            Store::test(t.0.clone()).unwrap().load().unwrap(),
            Loaded::Blocked(_)
        ));
    }
    #[test]
    fn raw_invalid_and_future_data_are_not_rewritten() {
        for bytes in [
            b"{invalid".as_slice(),
            b"{\"schemaId\":\"cuckoo.tauri.settings\",\"schemaVersion\":8}",
            b"{\"version\":6}",
        ] {
            let t = Temp::new();
            let mut s = Store::test(t.0.clone()).unwrap();
            fs::write(s.path(), bytes).unwrap();
            assert!(matches!(s.load().unwrap(), Loaded::Blocked(_)));
            assert_eq!(fs::read(s.path()).unwrap(), bytes);
        }
    }
    #[test]
    fn retention_and_old_backup_restore_use_new_generation() {
        let t = Temp::new();
        let mut s = Store::test(t.0.clone()).unwrap();
        s.load().unwrap();
        for volume in 0..9 {
            let mut d = Document::default();
            d.audio.volume = volume;
            s.commit(d).unwrap();
        }
        let c = s.candidates().unwrap();
        assert_eq!(c.len(), 5);
        assert_eq!(c[0].generation, 8);
        assert_eq!(c[4].generation, 4);
        drop(s);
        fs::write(t.0.join("settings.json"), b"{corrupt").unwrap();
        let mut s = Store::test(t.0.clone()).unwrap();
        s.load().unwrap();
        let restored = s.restore(&c[4].token).unwrap();
        assert_eq!(restored.generation, 9);
        assert_eq!(restored.audio.volume, 3);
    }
}
