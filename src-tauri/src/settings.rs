//! Rust-owned MemorySettings survive WebView lifetimes; one worker persists them.
//! Neither IPC nor startup arguments can choose a disk path.
#[cfg(test)]
#[path = "custom_chime_tests.rs"]
mod custom_chime_tests;
#[cfg(test)]
#[path = "settings_e1_tests.rs"]
mod e1_tests;
#[cfg(test)]
#[path = "settings_e2_tests.rs"]
mod e2_tests;
use crate::{
    settings_disk::{Candidate, Loaded, Store},
    settings_model::{Document, MAX_COUNTER},
};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
pub const APP_IDENTIFIER: &str = "jp.hiroyamahiroshi.cuckoo";
pub const DEV_IDENTIFIER: &str = "jp.hiroyamahiroshi.cuckoo.dev";
pub fn storage_identifier() -> &'static str {
    if cfg!(debug_assertions) { DEV_IDENTIFIER } else { APP_IDENTIFIER }
}
pub fn check_identifier(id: &str) -> Result<(), &'static str> {
    #[cfg(debug_assertions)]
    if id == DEV_IDENTIFIER { return Ok(()); }
    // Verification-only identity avoids interfering with a running dev build.
    // This feature also fixes storage to its embedded workspace UUID root.
    #[cfg(feature = "ui-registration-test-harness")]
    if id == "jp.hiroyamahiroshi.cuckoo.ui-registration-verification" {
        return Ok(());
    }
    if id == APP_IDENTIFIER {
        Ok(())
    } else {
        Err("Application identifier mismatch")
    }
}
#[cfg_attr(feature = "settings-test-harness", allow(dead_code))]
pub fn reserved_settings_path(root: Option<&Path>) -> Result<PathBuf, &'static str> {
    let root = root.ok_or("Prototype local data root is unavailable")?;
    if !root.is_absolute() || root.components().any(|p| matches!(p, Component::ParentDir)) {
        return Err("Invalid prototype root");
    }
    Ok(root.join(storage_identifier()).join("settings.json"))
}
struct Request {
    id: String,
    base: u64,
    patch: Value,
    at: Instant,
}
struct PresetReceipt {
    request_id: String,
    base: u64,
    action: Value,
    outcome: Value,
    at: Instant,
}
struct PresetJob {
    request_id: String,
    doc: Document,
    id: String,
}
pub(crate) struct StorageJob {
    operation: Box<
        dyn FnOnce(&mut Store, &Document) -> Result<Document, crate::settings_disk::DiskError>
            + Send,
    >,
    reply: std::sync::mpsc::SyncSender<Result<(), crate::settings_disk::DiskError>>,
}
struct Inner {
    doc: Document,
    storage_job: Option<StorageJob>,
    session: String,
    revision: u64,
    saved: Option<u64>,
    seq: u64,
    mode: &'static str,
    save: &'static str,
    error: String,
    loaded: bool,
    scheduled: bool,
    due: Instant,
    in_flight: bool,
    stop: bool,
    closing: bool,
    protected_start: bool,
    availability: Value,
    candidates: Vec<Candidate>,
    can_restore: bool,
    restore: Option<String>,
    restoring: bool,
    requests: VecDeque<Request>,
    preset_receipts: VecDeque<PresetReceipt>,
    preset_job: Option<PresetJob>,
    preset_busy: bool,
}
pub struct MemorySettings {
    inner: Mutex<Inner>,
    wake: Condvar,
    started: AtomicBool,
}
impl Default for MemorySettings {
    fn default() -> Self {
        let mut doc = Document::default();
        doc.schedule.enabled = false;
        Self {
            inner: Mutex::new(Inner {
                doc,
                storage_job: None,
                session: Uuid::new_v4().to_string(),
                revision: 0,
                saved: None,
                seq: 0,
                mode: "loading",
                save: "initial",
                error: String::new(),
                loaded: false,
                scheduled: false,
                due: Instant::now(),
                in_flight: false,
                stop: false,
                closing: false,
                protected_start: false,
                availability: json!({}),
                candidates: vec![],
                can_restore: false,
                restore: None,
                restoring: false,
                requests: VecDeque::new(),
                preset_receipts: VecDeque::new(),
                preset_job: None,
                preset_busy: false,
            }),
            wake: Condvar::new(),
            started: AtomicBool::new(false),
        }
    }
}
fn metadata(i: &Inner) -> Value {
    json!({"sessionId":i.session,"revision":i.revision,"savedRevision":i.saved,"eventSeq":i.seq,"generation":i.doc.generation,"schemaVersion":i.doc.schema_version,"catalogRef":i.doc.catalog_ref,
        "mode":i.mode,"loaded":i.loaded,"canEdit":i.mode=="persistent"&&!i.restoring&&!i.closing&&!i.preset_busy,"canRestore":i.can_restore&&!i.in_flight&&!i.restoring&&!i.closing&&!i.preset_busy,
        "presetOperation":i.preset_receipts.back().map(|r|json!({"requestId":r.request_id,"outcome":r.outcome})),
        "errorCode":i.error,"backupCandidates":i.candidates.iter().map(|c|json!({"token":c.token,"generation":c.generation})).collect::<Vec<_>>()})
}
impl MemorySettings {
    pub(crate) fn catalog_reference(
        &self,
    ) -> Result<Option<crate::settings_model::CatalogRef>, String> {
        let i = self.inner.lock().map_err(|_| "SettingsPoisoned")?;
        if i.mode != "persistent" {
            return Err("SettingsReadBlocked".into());
        }
        Ok(i.doc.catalog_ref.clone())
    }
    pub(crate) fn check_revision(&self, session: &str, base: u64) -> Result<(), String> {
        let i = self.inner.lock().map_err(|_| "SettingsPoisoned")?;
        if session != i.session {
            return Err("SessionConflict".into());
        }
        if base != i.revision {
            return Err("RevisionConflict".into());
        }
        if i.mode != "persistent"
            || i.saved != Some(i.revision)
            || i.in_flight
            || i.preset_busy
            || i.restoring
            || i.closing
            || i.stop
        {
            return Err("SettingsWriteBlockedOrDirty".into());
        }
        Ok(())
    }
    pub(crate) fn storage_operation_at(
        &self,
        expected: Option<(&str, u64)>,
        operation: impl FnOnce(&mut Store, &Document) -> Result<Document, crate::settings_disk::DiskError>
            + Send
            + 'static,
    ) -> Result<std::sync::mpsc::Receiver<Result<(), crate::settings_disk::DiskError>>, String>
    {
        let mut i = self.inner.lock().unwrap();
        if let Some((session, revision)) = expected {
            if session != i.session {
                return Err("SessionConflict".into());
            }
            if revision != i.revision {
                return Err("RevisionConflict".into());
            }
        }
        if i.revision >= MAX_COUNTER || i.seq >= MAX_COUNTER {
            return Err("RevisionExhausted".into());
        }
        if i.mode != "persistent"
            || i.in_flight
            || i.saved != Some(i.revision)
            || i.preset_busy
            || i.restoring
            || i.closing
            || i.stop
        {
            return Err("SettingsWriteBlockedOrDirty".into());
        }
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        i.storage_job = Some(StorageJob {
            operation: Box::new(operation),
            reply: tx,
        });
        i.preset_busy = true;
        self.wake.notify_all();
        Ok(rx)
    }
    pub fn select_sound(
        &self,
        session: &str,
        base: u64,
        request_id: &str,
        target: &str,
        sound_id: &str,
    ) -> Result<(), String> {
        self.submit_change(
            session,
            base,
            request_id,
            &json!({"type":"select-sound","target":target,"soundId":sound_id}),
        )
    }

    pub fn snapshot(&self) -> Value {
        self.inner.lock().unwrap().doc.projection()
    }
    pub fn status(&self) -> Value {
        let i = self.inner.lock().unwrap();
        let notice = if !i.loaded {
            "設定を読み込み中です。".into()
        } else if !i.error.is_empty() {
            format!(
                "設定の保存・読込エラー：{}。原本を保護しています。",
                i.error
            )
        } else if i.preset_busy {
            "設定操作を保存中です。完了までは変更を確定していません。".into()
        } else if i.save == "pending" {
            "設定は適用済み・未保存です。".into()
        } else {
            String::new()
        };
        json!({"settings":i.doc.projection(),"audioRoutingAvailability":i.availability,"settingsSync":metadata(&i),"saveState":i.save,"settingsNotice":notice})
    }
    pub fn apply(&self, session: &str, base: u64, id: &str, patch: &Value) -> Result<(), String> {
        if patch.get("customChimes").is_some() {
            return self.submit_change(session, base, id, &json!({"type":"custom-chimes","patch":patch}));
        }
        let mut i = self.inner.lock().unwrap();
        if session != i.session {
            return Err("SessionConflict".into());
        }
        if id.is_empty() || id.len() > 128 {
            return Err("InvalidRequestId".into());
        }
        if i.preset_receipts.iter().any(|r| r.request_id == id) {
            return Err("RequestIdReused".into());
        }
        while i
            .requests
            .front()
            .map(|r| r.at.elapsed() > Duration::from_secs(600))
            .unwrap_or(false)
        {
            i.requests.pop_front();
        }
        if let Some(old) = i.requests.iter().find(|r| r.id == id) {
            return if old.base == base && old.patch == *patch {
                Ok(())
            } else {
                Err("RequestIdReused".into())
            };
        }
        if base != i.revision {
            return Err("RevisionConflict".into());
        }
        if i.mode != "persistent" || i.restoring || i.stop || i.closing || i.preset_busy {
            return Err("SettingsWriteBlocked".into());
        }
        if i.revision >= MAX_COUNTER || i.seq >= MAX_COUNTER {
            return Err("RevisionExhausted".into());
        }
        let doc = i.doc.patched(patch)?;
        i.doc = doc;
        i.revision += 1;
        i.seq += 1;
        i.save = "pending";
        i.error.clear();
        i.scheduled = true;
        i.due = Instant::now() + Duration::from_millis(400);
        i.requests.push_back(Request {
            id: id.into(),
            base,
            patch: patch.clone(),
            at: Instant::now(),
        });
        if i.requests.len() > 256 {
            i.requests.pop_front();
        }
        self.wake.notify_all();
        Ok(())
    }
    /// Disk-first for named operations: neither list nor clock changes until the
    /// single writer verifies the commit. A timeout never releases this gate.
    pub fn submit_preset(
        &self,
        session: &str,
        base: u64,
        request_id: &str,
        action: &Value,
    ) -> Result<(), String> {
        if action["type"] == "select-sound" || action["type"] == "custom-chimes" {
            return Err("InvalidPresetAction".into());
        }
        self.submit_change(session, base, request_id, action)
    }
    fn submit_change(
        &self,
        session: &str,
        base: u64,
        request_id: &str,
        action: &Value,
    ) -> Result<(), String> {
        let mut i = self.inner.lock().unwrap();
        if session != i.session {
            return Err("SessionConflict".into());
        }
        if request_id.is_empty() || request_id.len() > 128 {
            return Err("InvalidRequestId".into());
        }
        while i
            .preset_receipts
            .front()
            .map(|r| r.at.elapsed() > Duration::from_secs(600) && r.outcome["state"] != "pending")
            .unwrap_or(false)
        {
            i.preset_receipts.pop_front();
        }
        if i.requests.iter().any(|r| r.id == request_id) {
            return Err("RequestIdReused".into());
        }
        if let Some(r) = i
            .preset_receipts
            .iter()
            .find(|r| r.request_id == request_id)
        {
            return if r.base == base && r.action == *action {
                Ok(())
            } else {
                Err("RequestIdReused".into())
            };
        }
        if base != i.revision {
            return Err("RevisionConflict".into());
        }
        if i.mode != "persistent" || i.restoring || i.stop || i.closing || i.preset_busy {
            return Err("SettingsWriteBlocked".into());
        }
        if i.revision >= MAX_COUNTER || i.seq >= MAX_COUNTER {
            return Err("RevisionExhausted".into());
        }
        let (doc, id) = if action["type"] == "select-sound" {
            (
                i.doc.select_sound(
                    action["target"].as_str().ok_or("InvalidTarget")?,
                    action["soundId"].as_str().ok_or("InvalidSoundId")?,
                )?,
                action["soundId"].as_str().unwrap().to_string(),
            )
        } else if action["type"] == "custom-chimes" {
            (i.doc.patched(&action["patch"])?, String::new())
        } else {
            i.doc.edit_preset(action)?
        };
        i.preset_receipts.push_back(PresetReceipt {
            request_id: request_id.into(),
            base,
            action: action.clone(),
            outcome: json!({"state":"pending"}),
            at: Instant::now(),
        });
        if i.preset_receipts.len() > 256 {
            i.preset_receipts.pop_front();
        }
        i.preset_job = Some(PresetJob {
            request_id: request_id.into(),
            doc,
            id,
        });
        i.preset_busy = true;
        i.seq += 1;
        self.wake.notify_all();
        Ok(())
    }
    pub fn wait_preset(&self, request_id: &str, timeout: Duration) -> Value {
        let end = Instant::now() + timeout;
        let mut i = self.inner.lock().unwrap();
        loop {
            let Some(r) = i
                .preset_receipts
                .iter()
                .find(|r| r.request_id == request_id)
            else {
                return json!({"state":"failed","code":"OperationExpired"});
            };
            if r.outcome["state"] != "pending" {
                return r.outcome.clone();
            }
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return json!({"state":"pending","code":"保存結果は未確定です。同じ操作の再試行で結果を確認してください。"});
            }
            i = self.wake.wait_timeout(i, left).unwrap().0;
        }
    }
    pub fn toggle(&self) -> Result<(), String> {
        let (session, rev, enabled) = {
            let i = self.inner.lock().unwrap();
            (i.session.clone(), i.revision, !i.doc.schedule.enabled)
        };
        self.apply(
            &session,
            rev,
            &Uuid::new_v4().to_string(),
            &json!({"enabled":enabled}),
        )
    }
    pub fn retry(&self) -> Result<(), String> {
        let mut i = self.inner.lock().unwrap();
        if i.mode != "persistent" || i.restoring || i.preset_busy {
            return Err("SettingsWriteBlocked".into());
        }
        i.error.clear();
        i.save = "pending";
        i.scheduled = true;
        i.due = Instant::now();
        i.seq += 1;
        self.wake.notify_all();
        Ok(())
    }
    pub fn restore(&self, session: &str, base: u64, token: &str) -> Result<(), String> {
        let mut i = self.inner.lock().unwrap();
        if session != i.session || base != i.revision {
            return Err("RevisionConflict".into());
        }
        if !i.can_restore
            || i.in_flight
            || i.restoring
            || i.closing
            || i.preset_busy
            || i.revision >= MAX_COUNTER
            || !i.candidates.iter().any(|c| c.token == token)
        {
            return Err("RecoveryUnavailable".into());
        }
        i.restore = Some(token.into());
        i.restoring = true;
        i.scheduled = false;
        i.seq += 1;
        self.wake.notify_all();
        Ok(())
    }
    pub fn start(
        self: &Arc<Self>,
        notify: impl Fn() + Send + 'static,
    ) -> Result<thread::JoinHandle<()>, String> {
        self.start_with(Store::prototype, notify)
    }
    fn start_with(
        self: &Arc<Self>,
        open: impl FnOnce() -> Result<Store, crate::settings_disk::DiskError> + Send + 'static,
        notify: impl Fn() + Send + 'static,
    ) -> Result<thread::JoinHandle<()>, String> {
        if self.started.swap(true, Ordering::SeqCst) {
            return Err("SettingsWorkerAlreadyStarted".into());
        }
        let state = self.clone();
        thread::Builder::new()
            .name("cuckoo-settings".into())
            .spawn(move || state.worker(open, notify))
            .map_err(|e| {
                self.started.store(false, Ordering::SeqCst);
                let mut i = self.inner.lock().unwrap();
                i.loaded = true;
                i.mode = "blocked";
                i.save = "error";
                i.error = "SettingsWorkerUnavailable".into();
                i.protected_start = true;
                i.seq += 1;
                e.to_string()
            })
    }
    fn worker(
        &self,
        open: impl FnOnce() -> Result<Store, crate::settings_disk::DiskError>,
        notify: impl Fn(),
    ) {
        let mut store = match open() {
            Ok(mut s) => {
                let result = s.load();
                let availability = match &result {
                    Ok(Loaded::Valid(d)) => s.availability(d),
                    _ => json!({}),
                };
                let candidates = s.candidates().unwrap_or_default();
                let mut i = self.inner.lock().unwrap();
                i.loaded = true;
                i.availability = availability;
                i.seq += 1;
                i.candidates = candidates;
                match result {
                    Ok(Loaded::Valid(d)) => {
                        i.doc = d;
                        i.mode = "persistent";
                        i.save = "saved";
                        i.saved = Some(0);
                        i.can_restore = true;
                    }
                    Ok(Loaded::Fresh) => {
                        i.doc = Document::default();
                        i.mode = "persistent";
                        i.save = "pending";
                        i.scheduled = true;
                        i.due = Instant::now();
                        i.can_restore = true;
                    }
                    Ok(Loaded::Blocked(e)) => {
                        i.can_restore = !e.starts_with("Unsupported");
                        i.error = e;
                        i.mode = "blocked";
                        i.save = "error";
                        i.protected_start = true;
                    }
                    Err(e) => {
                        i.error = e.code;
                        i.mode = "blocked";
                        i.save = "error";
                        i.protected_start = true;
                    }
                }
                drop(i);
                Some(s)
            }
            Err(e) => {
                let mut i = self.inner.lock().unwrap();
                i.loaded = true;
                i.seq += 1;
                i.mode = "blocked";
                i.save = "error";
                i.error = e.code;
                i.protected_start = true;
                None
            }
        };
        self.wake.notify_all();
        notify();
        loop {
            let mut i = self.inner.lock().unwrap();
            while !i.stop
                && i.restore.is_none()
                && i.preset_job.is_none()
                && i.storage_job.is_none()
                && !(i.scheduled && i.mode == "persistent" && Instant::now() >= i.due)
            {
                if i.scheduled && i.mode == "persistent" {
                    let wait = i.due.saturating_duration_since(Instant::now());
                    i = self.wake.wait_timeout(i, wait).unwrap().0;
                } else {
                    i = self.wake.wait(i).unwrap();
                }
            }
            if i.stop {
                return;
            }
            if let Some(job) = i.storage_job.take() {
                let doc = i.doc.clone();
                i.in_flight = true;
                drop(i);
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match store
                    .as_mut()
                {
                    Some(s) => (job.operation)(s, &doc),
                    None => Err(crate::settings_disk::DiskError {
                        code: "SettingsUnavailable".into(),
                        uncertain: true,
                    }),
                }))
                .unwrap_or_else(|_| {
                    Err(crate::settings_disk::DiskError {
                        code: "StorageOperationPanicked".into(),
                        uncertain: true,
                    })
                });
                let availability = result
                    .as_ref()
                    .ok()
                    .and_then(|d| store.as_ref().map(|s| s.availability(d)));
                let candidates = store
                    .as_ref()
                    .and_then(|s| s.candidates().ok())
                    .unwrap_or_default();
                let mut i = self.inner.lock().unwrap();
                i.candidates = candidates;
                i.in_flight = false;
                i.preset_busy = false;
                i.seq += 1;
                if let Some(a) = availability {
                    i.availability = a;
                }
                let ack = match result {
                    Ok(doc) => {
                        if i.doc != doc {
                            i.doc = doc;
                            i.revision += 1;
                        }
                        i.saved = Some(i.revision);
                        i.save = "saved";
                        i.error.clear();
                        Ok(())
                    }
                    Err(e) => {
                        i.error = e.code.clone();
                        i.save = "error";
                        if e.uncertain {
                            i.mode = "blocked";
                            i.can_restore = false;
                        }
                        Err(e)
                    }
                };
                drop(i);
                self.wake.notify_all();
                notify();
                let _ = job.reply.send(ack);
                continue;
            }
            if i.preset_job.is_some() && i.mode != "persistent" {
                let job = i.preset_job.take().unwrap();
                let code = i.error.clone();
                if let Some(r) = i
                    .preset_receipts
                    .iter_mut()
                    .find(|r| r.request_id == job.request_id)
                {
                    r.outcome = json!({"state":"failed","code":code,"uncertain":false});
                }
                i.preset_busy = false;
                i.seq += 1;
                drop(i);
                self.wake.notify_all();
                notify();
                continue;
            }
            let preset = i.preset_job.take();
            let token = i.restore.take();
            let doc = preset
                .as_ref()
                .map(|j| j.doc.clone())
                .unwrap_or_else(|| i.doc.clone());
            let revision = i.revision;
            i.scheduled = false;
            i.in_flight = true;
            i.save = "pending";
            i.seq += 1;
            drop(i);
            notify();
            let Some(s) = store.as_mut() else { continue };
            let result = if let Some(token) = &token {
                s.restore(token)
            } else {
                if let Some(job) = &preset {
                    if crate::settings_model::sound_id_valid(&job.id) {
                        s.validate_sound(&job.id).and_then(|_| s.commit(doc))
                    } else {
                        s.commit(doc)
                    }
                } else {
                    s.commit(doc)
                }
            };
            let availability = result.as_ref().ok().map(|d| s.availability(d));
            let candidates = s.candidates().unwrap_or_default();
            let mut i = self.inner.lock().unwrap();
            i.in_flight = false;
            if let Some(a) = availability {
                i.availability = a;
            }
            i.restoring = false;
            i.candidates = candidates;
            i.seq += 1;
            match result {
                Ok(doc) => {
                    if let Some(job) = &preset {
                        i.doc = doc;
                        i.revision += 1;
                        i.saved = Some(i.revision);
                        i.preset_busy = false;
                        let outcome = json!({"state":"saved","id":job.id,"revision":i.revision,"generation":i.doc.generation});
                        if let Some(r) = i
                            .preset_receipts
                            .iter_mut()
                            .find(|r| r.request_id == job.request_id)
                        {
                            r.outcome = outcome;
                        }
                    } else if token.is_some() {
                        i.doc = doc;
                        i.revision += 1;
                        i.saved = Some(i.revision);
                        i.mode = "persistent";
                        i.protected_start = false;
                    } else {
                        i.doc.generation = doc.generation;
                        i.doc.catalog_ref = doc.catalog_ref;
                        i.saved = Some(revision);
                    }
                    i.error.clear();
                    i.save = if i.saved == Some(i.revision) && !i.preset_busy {
                        "saved"
                    } else {
                        "pending"
                    };
                    i.can_restore = true;
                }
                Err(e) => {
                    if let Some(job) = &preset {
                        i.preset_busy = false;
                        if let Some(r) = i
                            .preset_receipts
                            .iter_mut()
                            .find(|r| r.request_id == job.request_id)
                        {
                            r.outcome =
                                json!({"state":"failed","code":e.code,"uncertain":e.uncertain});
                        }
                    }
                    i.error = e.code;
                    i.save = "error";
                    i.scheduled = false;
                    if e.uncertain || i.error == "ExternalChangeConflict" {
                        i.mode = "blocked";
                        i.can_restore = false;
                    }
                }
            }
            drop(i);
            self.wake.notify_all();
            notify();
        }
    }
    /// Timeout does not stop I/O, unlock a writer, or claim discarded changes.
    pub fn flush(&self, timeout: Duration) -> Result<(), String> {
        let end = Instant::now() + timeout;
        let mut i = self.inner.lock().unwrap();
        if i.loaded && i.mode == "persistent" && !i.in_flight {
            i.scheduled = i.saved != Some(i.revision);
            i.due = Instant::now();
            self.wake.notify_all();
        }
        loop {
            if i.loaded && !i.in_flight && !i.restoring && !i.preset_busy {
                if i.save == "saved" || (i.mode == "persistent" && i.saved == Some(i.revision)) {
                    return Ok(());
                }
                if i.mode == "blocked" && i.protected_start && i.revision == 0 {
                    return Ok(());
                } // untouched protected startup file
                if !i.error.is_empty() {
                    return Err(i.error.clone());
                }
            }
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("SaveStillRunningOrPending".into());
            }
            i = self.wake.wait_timeout(i, left).unwrap().0;
            if i.loaded && i.scheduled {
                i.due = Instant::now();
                self.wake.notify_all();
            }
        }
    }
    pub fn stop_worker(&self) {
        let mut i = self.inner.lock().unwrap();
        i.stop = true;
        self.wake.notify_all();
    }
    pub fn closing(&self, value: bool) {
        let mut i = self.inner.lock().unwrap();
        i.closing = value;
        i.seq += 1;
        self.wake.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn preset(s: &MemorySettings, key: &str, action: Value) -> Value {
        let m = s.status()["settingsSync"].clone();
        s.submit_preset(
            m["sessionId"].as_str().unwrap(),
            m["revision"].as_u64().unwrap(),
            key,
            &action,
        )
        .unwrap();
        s.wait_preset(key, Duration::from_secs(5))
    }
    #[test]
    fn named_presets_are_durable_and_restore_on_restart() {
        let root = root();
        let (s, h) = boot(root.clone());
        let initial = s.snapshot();
        let made = preset(&s, "make", json!({"type":"create","name":"Morning"}));
        assert_eq!(made["state"], "saved");
        let id = made["id"].as_str().unwrap();
        let session = s.status()["settingsSync"]["sessionId"]
            .as_str()
            .unwrap()
            .to_string();
        s.submit_preset(
            &session,
            0,
            "make",
            &json!({"type":"create","name":"Morning"}),
        )
        .unwrap();
        assert_eq!(s.wait_preset("make", Duration::ZERO), made);
        assert_eq!(s.snapshot()["presets"].as_array().unwrap().len(), 1);
        assert!(s
            .submit_preset(&session, 0, "stale", &json!({"type":"delete","id":id}))
            .is_err());
        assert!(s
            .submit_preset("old-session", 1, "wrong", &json!({"type":"delete","id":id}))
            .is_err());
        assert!(s
            .submit_preset(
                &session,
                0,
                "make",
                &json!({"type":"create","name":"Changed"})
            )
            .is_err());
        assert!(s.apply(&session, 1, "make", &json!({"volume":1})).is_err());
        assert_eq!(
            preset(&s, "rename", json!({"type":"rename","id":id,"name":"Noon"}))["state"],
            "saved"
        );
        close(s, h);
        let (s, h) = boot(root.clone());
        assert_eq!(s.snapshot()["presets"][0]["name"], "Noon");
        let m = s.status()["settingsSync"].clone();
        s.apply(
            m["sessionId"].as_str().unwrap(),
            0,
            "slots",
            &json!({"enabled":false,"volume":37,"slots":vec![false;48]}),
        )
        .unwrap();
        assert_eq!(
            preset(&s, "apply", json!({"type":"apply","id":id}))["state"],
            "saved"
        );
        assert_eq!(s.snapshot()["slots"], initial["slots"]);
        assert_eq!(s.snapshot()["volume"], 37);
        assert_eq!(s.snapshot()["enabled"], false);
        assert_eq!(
            preset(&s, "overwrite", json!({"type":"overwrite","id":id}))["state"],
            "saved"
        );
        assert_eq!(
            preset(&s, "delete", json!({"type":"delete","id":id}))["state"],
            "saved"
        );
        assert!(s.snapshot()["presets"].as_array().unwrap().is_empty());
        close(s, h);
        let (s, h) = boot(root.clone());
        assert!(s.snapshot()["presets"].as_array().unwrap().is_empty());
        close(s, h);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn preset_save_failures_never_publish_or_ack_success() {
        use crate::settings_disk::Stage;
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
            let root = root();
            let mut store = Store::test(root.clone()).unwrap();
            store.load().unwrap();
            store.commit(Document::default()).unwrap();
            store.fault = Some(point);
            let s = Arc::new(MemorySettings::default());
            let h = s.start_with(move || Ok(store), || {}).unwrap();
            s.flush(Duration::from_secs(5)).unwrap();
            let before = s.snapshot();
            let result = preset(
                &s,
                "failed-create",
                json!({"type":"create","name":"Not confirmed"}),
            );
            assert_eq!(result["state"], "failed");
            assert_eq!(s.snapshot(), before);
            assert_eq!(s.status()["settingsSync"]["revision"], 0);
            let uncertain = matches!(
                point,
                Stage::Replace | Stage::AfterReplace | Stage::FinalVerify
            );
            assert_eq!(result["uncertain"], uncertain);
            assert_eq!(s.flush(Duration::from_secs(1)).is_err(), uncertain);
            assert_eq!(s.wait_preset("failed-create", Duration::ZERO), result);
            close(s, h);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn pending_preset_gate_survives_timeout_and_duplicate_request() {
        use crate::settings_disk::Stage;
        let root = root();
        let mut store = Store::test(root.clone()).unwrap();
        store.load().unwrap();
        store.commit(Document::default()).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let (resume, wait) = std::sync::mpsc::channel();
        store.gate = Some((Stage::FinalVerify, tx, wait));
        let s = Arc::new(MemorySettings::default());
        let h = s.start_with(move || Ok(store), || {}).unwrap();
        s.flush(Duration::from_secs(5)).unwrap();
        let session = s.status()["settingsSync"]["sessionId"]
            .as_str()
            .unwrap()
            .to_string();
        let action = json!({"type":"create","name":"Once"});
        s.submit_preset(&session, 0, "one", &action).unwrap();
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(s.snapshot()["presets"].as_array().unwrap().is_empty());
        assert_eq!(s.status()["settingsSync"]["canEdit"], false);
        assert_eq!(
            s.wait_preset("one", Duration::from_millis(5))["state"],
            "pending"
        );
        s.submit_preset(&session, 0, "one", &action).unwrap();
        assert!(s.submit_preset(&session, 0, "two", &action).is_err());
        assert!(s.apply(&session, 0, "late", &json!({"volume":12})).is_err());
        assert!(s.flush(Duration::from_millis(5)).is_err());
        resume.send(()).unwrap();
        assert_eq!(
            s.wait_preset("one", Duration::from_secs(5))["state"],
            "saved"
        );
        assert_eq!(s.snapshot()["presets"].as_array().unwrap().len(), 1);
        close(s, h);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn invalid_preset_file_is_protected_without_electron_style_row_dropping() {
        let root = root();
        std::fs::create_dir_all(&root).unwrap();
        let mut v = serde_json::to_value(Document::default()).unwrap();
        v["presets"] = json!([{"id":"invalid","name":"Bad","slots":[true]}]);
        let raw = serde_json::to_vec(&v).unwrap();
        std::fs::write(root.join("settings.json"), &raw).unwrap();
        let (s, h) = boot(root.clone());
        assert_eq!(s.snapshot()["enabled"], false);
        let session = s.status()["settingsSync"]["sessionId"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(s
            .submit_preset(
                &session,
                0,
                "new",
                &json!({"type":"create","name":"No overwrite"})
            )
            .is_err());
        assert_eq!(std::fs::read(root.join("settings.json")).unwrap(), raw);
        close(s, h);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn preset_queued_behind_normal_commit_keeps_latest_state_or_cancels_on_uncertainty() {
        use crate::settings_disk::Stage;
        for fail in [false, true] {
            let root = root();
            let mut store = Store::test(root.clone()).unwrap();
            store.load().unwrap();
            store.commit(Document::default()).unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            let (resume, wait) = std::sync::mpsc::channel();
            store.gate = Some((Stage::TempVerify, tx, wait));
            if fail {
                store.fault = Some(Stage::AfterReplace);
            }
            let s = Arc::new(MemorySettings::default());
            let h = s.start_with(move || Ok(store), || {}).unwrap();
            s.flush(Duration::from_secs(5)).unwrap();
            let session = s.status()["settingsSync"]["sessionId"]
                .as_str()
                .unwrap()
                .to_string();
            s.apply(&session, 0, "volume", &json!({"volume":37}))
                .unwrap();
            rx.recv_timeout(Duration::from_secs(5)).unwrap();
            s.submit_preset(
                &session,
                1,
                "queued",
                &json!({"type":"create","name":"Queued"}),
            )
            .unwrap();
            resume.send(()).unwrap();
            let outcome = s.wait_preset("queued", Duration::from_secs(5));
            assert_eq!(outcome["state"], if fail { "failed" } else { "saved" });
            assert_eq!(s.snapshot()["volume"], 37);
            assert_eq!(
                s.snapshot()["presets"].as_array().unwrap().len(),
                if fail { 0 } else { 1 }
            );
            close(s, h);
            let (s, h) = boot(root.clone());
            assert_eq!(s.snapshot()["volume"], 37);
            assert_eq!(
                s.snapshot()["presets"].as_array().unwrap().len(),
                if fail { 0 } else { 1 }
            );
            close(s, h);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
    fn boot(root: PathBuf) -> (Arc<MemorySettings>, thread::JoinHandle<()>) {
        let s = Arc::new(MemorySettings::default());
        let h = s.start_with(move || Store::test(root), || {}).unwrap();
        s.flush(Duration::from_secs(5)).unwrap();
        (s, h)
    }
    fn root() -> PathBuf {
        std::env::temp_dir().join(format!("cuckoo-t3b-service-{}", Uuid::new_v4()))
    }
    fn close(s: Arc<MemorySettings>, h: thread::JoinHandle<()>) {
        s.stop_worker();
        h.join().unwrap();
    }
    #[test]
    fn settings_survive_view_lifetimes_and_restart_with_cas() {
        let root = root();
        let (s, h) = boot(root.clone());
        let m = s.status()["settingsSync"].clone();
        let session = m["sessionId"].as_str().unwrap();
        s.apply(session, 0, "one", &json!({"volume":37})).unwrap();
        assert!(s.apply(session, 0, "stale", &json!({"volume":99})).is_err());
        s.apply(session, 0, "one", &json!({"volume":37})).unwrap();
        assert!(s.apply(session, 0, "one", &json!({"volume":99})).is_err());
        assert!(s
            .apply("previous-session", 1, "new", &json!({"volume":99}))
            .is_err());
        for _ in 0..40 {
            assert_eq!(s.status()["settings"]["volume"], 37)
        }
        s.flush(Duration::from_secs(5)).unwrap();
        assert_eq!(s.status()["settingsSync"]["savedRevision"], 1);
        close(s, h);
        let (s, h) = boot(root.clone());
        assert_eq!(s.snapshot()["volume"], 37);
        close(s, h);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn many_updates_coalesce() {
        let root = root();
        let (s, h) = boot(root.clone());
        let session = s.status()["settingsSync"]["sessionId"]
            .as_str()
            .unwrap()
            .to_string();
        for n in 0..100 {
            s.apply(&session, n, &format!("r{n}"), &json!({"volume":n%101}))
                .unwrap();
        }
        s.flush(Duration::from_secs(5)).unwrap();
        let m = s.status();
        assert_eq!(m["settingsSync"]["savedRevision"], 100);
        assert_eq!(m["settings"]["volume"], 99);
        assert!(m["settingsSync"]["generation"].as_u64().unwrap() < 10);
        assert!(s.start(|| {}).is_err());
        close(s, h);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn protected_file_kept_and_clock_off() {
        let root = root();
        std::fs::create_dir_all(&root).unwrap();
        let b = b"{\"schemaId\":\"cuckoo.tauri.settings\",\"schemaVersion\":8}";
        std::fs::write(root.join("settings.json"), b).unwrap();
        let (s, h) = boot(root.clone());
        assert_eq!(s.snapshot()["enabled"], false);
        assert_eq!(s.status()["settingsSync"]["canEdit"], false);
        assert_eq!(std::fs::read(root.join("settings.json")).unwrap(), b);
        close(s, h);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn namespace_and_defaults_remain() {
        assert!(reserved_settings_path(None).is_err());
        assert!(reserved_settings_path(Some(Path::new("relative"))).is_err());
        assert!(reserved_settings_path(Some(Path::new("C:/synthetic/../other"))).is_err());
        assert_eq!(
            reserved_settings_path(Some(Path::new("C:/synthetic"))).unwrap(),
            Path::new("C:/synthetic")
                .join(storage_identifier())
                .join("settings.json")
        );
        let config: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        check_identifier(config["identifier"].as_str().unwrap()).unwrap();
        assert!(check_identifier("cuckoo").is_err());
        assert!(check_identifier("jp.hiroyamahiroshi.cuckoo.ui-registration-other").is_err());
        assert_eq!(check_identifier("jp.hiroyamahiroshi.cuckoo.ui-registration-verification").is_ok(),
            cfg!(feature = "ui-registration-test-harness"));
        let d = Document::default();
        assert_eq!(d.schedule.slots.iter().filter(|v| **v).count(), 13);
        assert_eq!(d.audio.on_hour.repeat, 2);
        assert!(d.appearance.background_illustration);
    }
    #[test]
    fn change_during_commit_remains_pending_and_timeout_does_not_unlock_writer() {
        use crate::settings_disk::Stage;
        let root = root();
        let (mut store, doc) = {
            let mut s = Store::test(root.clone()).unwrap();
            s.load().unwrap();
            let d = s.commit(Document::default()).unwrap();
            (s, d)
        };
        let (arrived_tx, arrived_rx) = std::sync::mpsc::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        store.gate = Some((Stage::FinalVerify, arrived_tx, resume_rx));
        let s = Arc::new(MemorySettings::default());
        let weak = Arc::downgrade(&s);
        let observations = Arc::new(Mutex::new(vec![]));
        let observed = observations.clone();
        let h = s
            .start_with(
                move || Ok(store),
                move || {
                    if let Some(s) = weak.upgrade() {
                        observed.lock().unwrap().push(s.status());
                    }
                },
            )
            .unwrap();
        s.flush(Duration::from_secs(5)).unwrap();
        assert_eq!(s.snapshot()["volume"], doc.audio.volume);
        let session = s.status()["settingsSync"]["sessionId"]
            .as_str()
            .unwrap()
            .to_string();
        s.apply(&session, 0, "one", &json!({"volume":37})).unwrap();
        arrived_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(s.flush(Duration::from_millis(10)).is_err());
        assert!(s.inner.lock().unwrap().in_flight);
        s.apply(&session, 1, "two", &json!({"volume":38})).unwrap();
        resume_tx.send(()).unwrap();
        s.flush(Duration::from_secs(5)).unwrap();
        assert!(observations
            .lock()
            .unwrap()
            .iter()
            .any(|v| v["settingsSync"]["savedRevision"] == 1
                && v["settingsSync"]["revision"] == 2
                && v["saveState"] == "pending"));
        assert_eq!(s.status()["settingsSync"]["savedRevision"], 2);
        assert_eq!(s.snapshot()["volume"], 38);
        s.closing(true);
        assert!(s.apply(&session, 2, "late", &json!({"volume":99})).is_err());
        s.closing(false);
        close(s, h);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn ordinary_save_failure_keeps_memory_and_explicit_retry_succeeds() {
        use crate::settings_disk::Stage;
        let root = root();
        let mut store = Store::test(root.clone()).unwrap();
        store.load().unwrap();
        store.commit(Document::default()).unwrap();
        store.fault = Some(Stage::TempFlush);
        let s = Arc::new(MemorySettings::default());
        let h = s.start_with(move || Ok(store), || {}).unwrap();
        s.flush(Duration::from_secs(5)).unwrap();
        let session = s.status()["settingsSync"]["sessionId"]
            .as_str()
            .unwrap()
            .to_string();
        s.apply(&session, 0, "one", &json!({"volume":37})).unwrap();
        // Observe worker failure, without flush's intentional retry request.
        let end = Instant::now() + Duration::from_secs(5);
        while s.status()["saveState"] != "error" {
            assert!(Instant::now() < end);
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(s.snapshot()["volume"], 37);
        assert_eq!(s.status()["settingsSync"]["savedRevision"], 0);
        s.retry().unwrap();
        s.flush(Duration::from_secs(5)).unwrap();
        close(s, h);
        let (s, h) = boot(root.clone());
        assert_eq!(s.snapshot()["volume"], 37);
        close(s, h);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn service_restore_and_ambiguous_commit_block_writes() {
        use crate::settings_disk::Stage;
        let root = root();
        let mut store = Store::test(root.clone()).unwrap();
        store.load().unwrap();
        store.commit(Document::default()).unwrap();
        store.commit(Document::default()).unwrap();
        drop(store);
        std::fs::write(root.join("settings.json"), b"{broken").unwrap();
        let (s, h) = boot(root.clone());
        let m = s.status()["settingsSync"].clone();
        s.restore(
            m["sessionId"].as_str().unwrap(),
            0,
            m["backupCandidates"][0]["token"].as_str().unwrap(),
        )
        .unwrap();
        s.flush(Duration::from_secs(5)).unwrap();
        assert_eq!(s.status()["settingsSync"]["mode"], "persistent");
        close(s, h);
        let mut store = Store::test(root.clone()).unwrap();
        store.load().unwrap();
        store.fault = Some(Stage::AfterReplace);
        let s = Arc::new(MemorySettings::default());
        let h = s.start_with(move || Ok(store), || {}).unwrap();
        s.flush(Duration::from_secs(5)).unwrap();
        let session = s.status()["settingsSync"]["sessionId"]
            .as_str()
            .unwrap()
            .to_string();
        s.apply(&session, 0, "change", &json!({"volume":37}))
            .unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        while s.status()["saveState"] != "error" {
            assert!(Instant::now() < end);
            thread::sleep(Duration::from_millis(10));
        }
        assert!(s.retry().is_err());
        assert!(s
            .apply(&session, 1, "later", &json!({"volume":99}))
            .is_err());
        assert!(s.flush(Duration::from_millis(10)).is_err());
        close(s, h);
        let (s, h) = boot(root.clone());
        assert_eq!(s.snapshot()["volume"], 37);
        close(s, h);
        std::fs::remove_dir_all(root).unwrap();
    }
}
