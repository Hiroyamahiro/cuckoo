use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs::{self, OpenOptions}, io::{Read, Write}, path::Path, process::{Command, Stdio}, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, thread, time::{Duration, Instant}};
use std::os::windows::{fs::{MetadataExt, OpenOptionsExt}, process::CommandExt};
use tauri::{Emitter, Manager};

pub const REPOSITORY: &str = "https://github.com/Hiroyamahiro/cuckoo";
const LATEST: &str = "https://api.github.com/repos/Hiroyamahiro/cuckoo/releases/latest";
const MAX_INSTALLER: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
pub struct Asset { pub name: String, pub size: u64, pub browser_download_url: String }
#[derive(Debug, Deserialize)]
pub struct Release { pub tag_name: String, pub draft: bool, pub prerelease: bool, pub assets: Vec<Asset> }
#[derive(Clone, Debug)]
pub struct Available { pub version: String, pub installer: Asset, pub checksums: Asset }
pub fn select(release: Release, current: &str) -> Result<Option<Available>, String> {
    if release.draft || release.prerelease { return Ok(None); }
    let remote = Version::parse(release.tag_name.strip_prefix('v').unwrap_or(&release.tag_name)).map_err(|_| "version")?;
    let current = Version::parse(current).map_err(|_| "version")?;
    if !remote.pre.is_empty() || !remote.build.is_empty() || !remote.cmp_precedence(&current).is_gt() { return Ok(None); }
    let name = format!("Cuckoo_{remote}_x64-setup.exe");
    let asset = |name: &str| -> Result<Asset, String> {
        let matches: Vec<_> = release.assets.iter().filter(|a| a.name == name).collect();
        if matches.len() != 1 { return Err("assets".into()); }
        let a = matches[0];
        if a.browser_download_url != format!("{REPOSITORY}/releases/download/{}/{name}", release.tag_name) { return Err("url".into()); }
        if a.size == 0 || a.size > if name == "SHA256SUMS.txt" { 65536 } else { MAX_INSTALLER } { return Err("size".into()); }
        Ok(a.clone())
    };
    Ok(Some(Available { version: remote.to_string(), installer: asset(&name)?, checksums: asset("SHA256SUMS.txt")? }))
}

pub fn checksum(text: &str, name: &str) -> Result<String, String> {
    let mut found = None;
    for line in text.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() == 2 && fields[1].trim_start_matches('*') == name {
            if found.is_some() || fields[0].len() != 64 || !fields[0].bytes().all(|c| c.is_ascii_hexdigit()) { return Err("integrity".into()); }
            found = Some(fields[0].to_ascii_lowercase());
        }
    }
    found.ok_or_else(|| "integrity".into())
}

fn download_file(path: &Path, size: u64, cancelled: &AtomicBool,
    transfer: impl FnOnce(&mut dyn FnMut(&[u8]) -> Result<(), String>) -> Result<u16, String>,
    mut progress: impl FnMut(u64)) -> Result<u64, String> {
    let mut out = OpenOptions::new().create_new(true).write(true).open(path).map_err(|_| "path")?;
    let mut count = 0u64;
    let code = transfer(&mut |chunk| {
        if cancelled.load(Ordering::SeqCst) { return Err("cancelled".into()); }
        if count + chunk.len() as u64 > size { return Err("size".into()); }
        out.write_all(chunk).map_err(|_| "write")?;
        count += chunk.len() as u64;
        progress(count);
        Ok(())
    })?;
    if cancelled.load(Ordering::SeqCst) { return Err("cancelled".into()); }
    if code != 200 || count != size { return Err("network".into()); }
    out.sync_all().map_err(|_| "write")?;
    Ok(count)
}

fn verify_installer(path: &Path, expected: &str) -> Result<fs::File, String> {
    let mut locked = OpenOptions::new().read(true).share_mode(1).open(path).map_err(|_| "path")?;
    let mut digest = Sha256::new();
    let mut chunk = [0u8; 65536];
    loop { let n = locked.read(&mut chunk).map_err(|_| "read")?; if n == 0 { break; } digest.update(&chunk[..n]); }
    if format!("{:x}", digest.finalize()) != expected { return Err("integrity".into()); }
    Ok(locked)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status { pub seq: u64, pub phase: String, pub version: Option<String>, pub received: u64, pub total: u64, pub message: String }
struct Inner { status: Status, release: Option<Available>, busy: bool }
fn finish_check(i: &mut Inner, result: Result<Option<Available>, String>, manual: bool) {
    i.busy = false;
    match result {
        Ok(release) => { i.release = release; i.status.version = i.release.as_ref().map(|r| r.version.clone()); i.status.message = if manual && i.release.is_none() { "最新のバージョンです".into() } else { String::new() }; }
        Err(_) if manual => i.status.message = "更新を確認できませんでした。再試行できます".into(),
        Err(_) => (),
    }
    i.status.phase = if i.release.is_some() { "available" } else { "idle" }.into();
}
pub struct Service { inner: Mutex<Inner>, cancel: AtomicBool }
impl Default for Service {
    fn default() -> Self { Self { inner: Mutex::new(Inner { status: Status { seq: 0, phase: "idle".into(), version: None, received: 0, total: 0, message: String::new() }, release: None, busy: false }), cancel: AtomicBool::new(false) } }
}
impl Service {
    pub fn status(&self) -> Status { self.inner.lock().unwrap().status.clone() }
    fn change(&self, app: &tauri::AppHandle, f: impl FnOnce(&mut Inner)) {
        let value = { let mut i = self.inner.lock().unwrap(); f(&mut i); i.status.seq += 1; i.status.clone() };
        let _ = app.emit("update-status", value);
    }
    fn begin(&self, app: &tauri::AppHandle, phase: &str) -> bool {
        let value = { let mut i = self.inner.lock().unwrap(); if i.busy { return false; } i.busy = true; i.status.phase = phase.into(); i.status.message.clear(); i.status.received = 0; i.status.total = 0; i.status.seq += 1; i.status.clone() };
        self.cancel.store(false, Ordering::SeqCst);
        let _ = app.emit("update-status", value); true
    }
    pub fn check(self: &Arc<Self>, app: tauri::AppHandle, manual: bool) {
        if !self.begin(&app, "checking") { return; }
        let service = self.clone();
        thread::spawn(move || {
            let result = (|| {
                let (code, data) = super::update_http::bytes(LATEST, 2 * 1024 * 1024)?;
                if code == 404 { return Ok(None); }
                if code != 200 { return Err("network".to_string()); }
                let release = serde_json::from_slice(&data).map_err(|_| "release".to_string())?;
                select(release, env!("CARGO_PKG_VERSION"))
            })();
            service.change(&app, |i| finish_check(i, result, manual));
        });
    }
    pub fn cancel(&self) { self.cancel.store(true, Ordering::SeqCst); }
    pub fn install(self: &Arc<Self>, app: tauri::AppHandle) -> Result<(), String> {
        let release = self.inner.lock().unwrap().release.clone().ok_or("更新が見つかっていません")?;
        if !self.begin(&app, "downloading") { return Ok(()); }
        let service = self.clone();
        thread::spawn(move || {
            let result = service.download_and_launch(&app, &release);
            service.change(&app, |i| {
                i.busy = false;
                i.status.phase = "available".into();
                i.status.message = match result.as_ref().map_err(String::as_str) {
                    Ok(_) => "インストーラーを終了しました。再試行できます".into(),
                    Err("cancelled") => "ダウンロードを中止しました。再試行できます".into(),
                    Err("integrity") => "ファイルの照合に失敗しました。再試行できます".into(),
                    Err("launch") => "インストーラーを起動できませんでした。再試行できます".into(),
                    Err(_) => "ダウンロードできませんでした。再試行できます".into(),
                };
            });
        });
        Ok(())
    }
    fn download_and_launch(&self, app: &tauri::AppHandle, release: &Available) -> Result<(), String> {
        let (code, sums) = super::update_http::bytes(&release.checksums.browser_download_url, 65536)?;
        if code != 200 || sums.len() as u64 != release.checksums.size { return Err("network".into()); }
        let expected = checksum(std::str::from_utf8(&sums).map_err(|_| "integrity")?, &release.installer.name)?;
        if self.cancel.load(Ordering::SeqCst) { return Err("cancelled".into()); }
        #[cfg(not(feature = "settings-test-harness"))]
        let base = app.path().app_cache_dir().map_err(|_| "path")?.join("updates");
        #[cfg(feature = "settings-test-harness")]
        let base = super::settings_disk::verification_root().join("updates");
        reject_reparse(&base)?;
        fs::create_dir_all(&base).map_err(|_| "path")?;
        let directory = base.join(uuid::Uuid::new_v4().to_string());
        fs::create_dir(&directory).map_err(|_| "path")?;
        let file = directory.join(&release.installer.name);
        let partial = directory.join("installer.part");
        let result = (|| {
            let mut last = Instant::now() - Duration::from_secs(1);
            let count = download_file(&partial, release.installer.size, &self.cancel,
              |receive| super::update_http::get(&release.installer.browser_download_url, release.installer.size, receive), |count| {
                if last.elapsed() >= Duration::from_millis(200) { self.change(app, |i| { i.status.received = count; i.status.total = release.installer.size; }); last = Instant::now(); }
            })?;
            if self.cancel.load(Ordering::SeqCst) { return Err("cancelled".into()); }
            fs::rename(&partial, &file).map_err(|_| "path")?;
            // Deny modification/deletion from verification through CreateProcess.
            let _locked = verify_installer(&file, &expected)?;
            if self.cancel.load(Ordering::SeqCst) { return Err("cancelled".into()); }
            // No inherited stdio, no wait on the UI thread, and no app-owned job:
            // Cuckoo shutdown does not terminate the independent installer.
            let mut installer = spawn_installer(&file)?;
            self.change(app, |i| { i.status.received = count; i.status.total = count; i.status.phase = "launched".into(); i.status.message = "インストーラーで更新を続けてください".into(); });
            // Waiting belongs to this worker only. Dropping Child never kills the
            // detached installer when Cuckoo exits, and no stdio pipe ties them.
            installer.wait().map_err(|_| "launch")?;
            Ok(())
        })();
        let _ = fs::remove_file(partial); let _ = fs::remove_file(file); let _ = fs::remove_dir(directory);
        result
    }
}

fn spawn_installer(file: &Path) -> Result<std::process::Child, String> {
    Command::new(file).current_dir(file.parent().ok_or("path")?).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .creation_flags(0x00000008 | 0x00000200).spawn().map_err(|_| "launch".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn release(version: &str) -> Release {
        let name = format!("Cuckoo_{version}_x64-setup.exe");
        Release { tag_name: format!("v{version}"), draft: false, prerelease: false,
            assets: [name, "SHA256SUMS.txt".into()].into_iter().map(|name| Asset { browser_download_url: format!("{REPOSITORY}/releases/download/v{version}/{name}"), name, size: 100 }).collect() }
    }
    #[test] fn stable_versions_compare_semantically_without_downgrades() {
        assert_eq!(select(release("2.0.10"), "2.0.9").unwrap().unwrap().version, "2.0.10");
        for v in ["1.9.9", "2.0.0", "2.0.1"] { assert!(select(release(v), "2.0.1").unwrap().is_none()); }
        assert!(select(release("2.1.0"), "2.0.99").unwrap().is_some());
    }
    #[test] fn prerelease_draft_and_invalid_asset_sets_are_never_installed() {
        for (draft, pre) in [(true,false),(false,true)] { let mut r = release("3.0.0"); r.draft = draft; r.prerelease = pre; assert!(select(r, "2.0.1").unwrap().is_none()); }
        assert!(select(release("3.0.0-rc.1"), "2.0.1").unwrap().is_none());
        let mut r=release("3.0.0"); r.assets[0].browser_download_url="https://github.com/other/repo/setup.exe".into(); assert!(select(r,"2.0.1").is_err());
        let mut r=release("3.0.0"); r.assets.pop(); assert!(select(r,"2.0.1").is_err());
        let mut r=release("3.0.0"); r.assets.push(r.assets[0].clone()); assert!(select(r,"2.0.1").is_err());
    }
    #[test] fn sums_must_identify_exactly_one_expected_filename() {
        let hash="ab".repeat(32); let name="Cuckoo_2.0.2_x64-setup.exe";
        assert_eq!(checksum(&format!("{hash}  {name}\r\n"), name).unwrap(),hash);
        assert_eq!(checksum(&format!("{hash} *{name}"), name).unwrap(),hash);
        for s in [format!("{hash} ../{name}"),format!("00 {name}"),format!("{hash} {name}\n{hash} {name}")] { assert!(checksum(&s,name).is_err()); }
    }
    #[test] fn automatic_failures_are_silent_and_manual_checks_are_retryable() {
        let service=Service::default();let mut state=service.inner.lock().unwrap();
        finish_check(&mut state,Err("network".into()),false);
        assert_eq!(state.status.phase,"idle");assert!(state.status.message.is_empty());assert!(!state.busy);
        finish_check(&mut state,Err("network".into()),true);
        assert!(!state.status.message.is_empty());assert!(!state.busy);
        finish_check(&mut state,Ok(None),true);
        assert_eq!(state.status.message,"最新のバージョンです");
        finish_check(&mut state,select(release("2.0.2"),"2.0.1"),false);
        assert_eq!(state.status.version.as_deref(),Some("2.0.2"));
        state.status.message.clear();finish_check(&mut state,Err("network".into()),false);
        assert_eq!(state.status.phase,"available");assert_eq!(state.status.version.as_deref(),Some("2.0.2"));assert!(state.status.message.is_empty());
    }
    #[test] fn interrupted_cancelled_corrupt_and_oversized_downloads_fail_before_launch() {
        let dir=Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("test-runtime").join(uuid::Uuid::new_v4().to_string()); fs::create_dir_all(&dir).unwrap();
        let cancel=AtomicBool::new(false);
        let good=dir.join("good.exe"); let hash=format!("{:x}",Sha256::digest(b"synthetic"));
        assert_eq!(download_file(&good,9,&cancel,|push|{push(b"syn")?;push(b"thetic")?;Ok(200)}, |_|{}).unwrap(),9);
        drop(verify_installer(&good,&hash).unwrap());
        assert!(verify_installer(&good,&"00".repeat(32)).is_err());
        for (name,mode) in [("interrupted",0),("short",1),("oversize",2),("cancel",3)] {
            cancel.store(false,Ordering::SeqCst);
            let result=download_file(&dir.join(name),9,&cancel,|push|{push(b"syn")?;match mode {0=>Err("network".into()),1=>Ok(200),2=>{push(b"too much data")?;Ok(200)},_=>{cancel.store(true,Ordering::SeqCst);push(b"thetic")?;Ok(200)}}}, |_|{});
            assert!(result.is_err(),"{name}");
        }
        fs::remove_dir_all(dir).unwrap();
    }
    #[test] fn redirects_are_https_and_restricted_to_github_delivery_hosts() {
        for u in ["http://github.com/a", "https://evil.example/a", "https://user@github.com/a", "https://github.com:444/a"] { assert!(super::super::update_http::allowed_url(u).is_err()); }
        assert!(super::super::update_http::allowed_url("https://release-assets.githubusercontent.com/a?token=synthetic").is_ok());
    }
    #[test] #[ignore = "read-only network smoke test; run explicitly"]
    fn official_release_transport() {
        let (status,body)=super::super::update_http::bytes(LATEST,2*1024*1024).unwrap();
        assert_eq!(status,200); let release:Release=serde_json::from_slice(&body).unwrap();
        // Check the real production transport, asset redirect, sums and binary hash
        // without launching anything, even when our local version is newer.
        let latest=select(release,"0.0.0").unwrap().unwrap();
        let (status,sums)=super::super::update_http::bytes(&latest.checksums.browser_download_url,65536).unwrap();assert_eq!(status,200);
        let expected=checksum(std::str::from_utf8(&sums).unwrap(),&latest.installer.name).unwrap();
        let mut digest=Sha256::new();let mut size=0;
        let status=super::super::update_http::get(&latest.installer.browser_download_url,MAX_INSTALLER,|chunk|{digest.update(chunk);size+=chunk.len() as u64;Ok(())}).unwrap();
        assert_eq!(status,200);assert_eq!(size,latest.installer.size);assert_eq!(format!("{:x}",digest.finalize()),expected);
    }
    #[test] #[ignore = "requires the benign installer fixture in ignored test-runtime"]
    fn detached_installer_fixture() {
        let fixture=Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("test-runtime/update-integration/DetachedFixture.exe");
        drop(spawn_installer(&fixture).unwrap());
        // The fixture writes its marker after this test executable has exited.
    }
}

fn reject_reparse(path: &Path) -> Result<(), String> {
    for p in path.ancestors() {
        match fs::symlink_metadata(p) { Ok(m) if m.file_attributes() & 0x400 != 0 => return Err("path".into()), Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err("path".into()), _ => () }
    }
    Ok(())
}

pub fn start(app: &tauri::AppHandle) {
    let app = app.clone();
    thread::spawn(move || {
        let mut next = Instant::now();
        loop {
            let state = app.state::<Arc<super::State>>();
            if state.stop.load(Ordering::SeqCst) { return; }
            if Instant::now() >= next { state.updates.check(app.clone(), false); next = Instant::now() + Duration::from_secs(24 * 3600); }
            thread::sleep(Duration::from_secs(1));
        }
    });
}

/// Must match setup/Update.cs. The per-installation semaphore blocks new starts
/// while the installer waits for graceful exit and replaces files.
pub fn installation_busy(exe: &Path) -> bool {
    use windows_sys::Win32::{Foundation::{CloseHandle, GetLastError, ERROR_FILE_NOT_FOUND}, System::Threading::{OpenSemaphoreW, ReleaseSemaphore, WaitForSingleObject, SEMAPHORE_MODIFY_STATE}};
    let root = exe.parent().unwrap_or(Path::new("")).to_string_lossy().trim_start_matches("\\\\?\\").replace('\\', "/").trim_end_matches('/').to_ascii_lowercase();
    let name = format!("Local\\CuckooUpdate-{:x}", Sha256::digest(root.as_bytes()));
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    unsafe { let handle = OpenSemaphoreW(0x00100000 | SEMAPHORE_MODIFY_STATE, 0, wide.as_ptr()); if handle.is_null() { return GetLastError() != ERROR_FILE_NOT_FOUND; } let result = WaitForSingleObject(handle, 0); if result == 0 { ReleaseSemaphore(handle, 1, std::ptr::null_mut()); } CloseHandle(handle); result != 0 }
}

#[tauri::command]
pub fn update_status(app: tauri::AppHandle, window: tauri::WebviewWindow) -> Result<Status, String> { super::current_window(&app, &window)?; Ok(app.state::<Arc<super::State>>().updates.status()) }
#[tauri::command]
pub fn update_action(app: tauri::AppHandle, window: tauri::WebviewWindow, action: String) -> Result<(), String> {
    super::current_window(&app, &window)?;
    let state = app.state::<Arc<super::State>>();
    if state.quit_pending.load(Ordering::SeqCst) { return Err("終了処理中です".into()); }
    match action.as_str() { "check" => state.updates.check(app.clone(), true), "install" => state.updates.install(app.clone())?, "cancel" => state.updates.cancel(), _ => return Err("Invalid update action".into()) }
    Ok(())
}
