//! Blocking WinHTTP runs only on the updater's worker, never the clock/UI thread.
use std::{ffi::c_void, ptr, time::{Duration, Instant}};
use windows_sys::Win32::Networking::WinHttp::*;

struct Handle(*mut c_void);
impl Handle {
    fn new(p: *mut c_void) -> Result<Self, String> {
        if p.is_null() { Err("network".into()) } else { Ok(Self(p)) }
    }
}
impl Drop for Handle { fn drop(&mut self) { unsafe { WinHttpCloseHandle(self.0); } } }
fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }

pub fn allowed_url(raw: &str) -> Result<tauri::Url, String> {
    let u = tauri::Url::parse(raw).map_err(|_| "url")?;
    if u.scheme() != "https" || !u.username().is_empty() || u.password().is_some()
        || u.port_or_known_default() != Some(443) || u.fragment().is_some()
        || !matches!(u.host_str(), Some("api.github.com" | "github.com" | "release-assets.githubusercontent.com" | "objects.githubusercontent.com")) {
        return Err("url".into());
    }
    Ok(u)
}

pub fn get(url: &str, limit: u64, mut receive: impl FnMut(&[u8]) -> Result<(), String>) -> Result<u16, String> {
    let started = Instant::now();
    let mut url = allowed_url(url)?;
    unsafe {
        let agent = wide(concat!("Cuckoo/", env!("CARGO_PKG_VERSION")));
        let session = Handle::new(WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, ptr::null(), ptr::null(), 0))?;
        if WinHttpSetTimeouts(session.0, 10_000, 10_000, 15_000, 15_000) == 0 { return Err("network".into()); }
        for _ in 0..6 {
            let host = wide(url.host_str().ok_or("url")?);
            let conn = Handle::new(WinHttpConnect(session.0, host.as_ptr(), 443, 0))?;
            let route = wide(&format!("{}{}", url.path(), url.query().map(|q| format!("?{q}")).unwrap_or_default()));
            let verb = wide("GET");
            let request = Handle::new(WinHttpOpenRequest(conn.0, verb.as_ptr(), route.as_ptr(), ptr::null(), ptr::null(), ptr::null(), WINHTTP_FLAG_SECURE))?;
            let policy = WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
            if WinHttpSetOption(request.0, WINHTTP_OPTION_REDIRECT_POLICY, &policy as *const _ as _, 4) == 0 { return Err("network".into()); }
            let accept = if url.host_str() == Some("api.github.com") { "application/vnd.github+json" } else { "application/octet-stream" };
            let headers = wide(&format!("Accept: {accept}\r\nX-GitHub-Api-Version: 2022-11-28\r\n"));
            if WinHttpSendRequest(request.0, headers.as_ptr(), (headers.len() - 1) as u32, ptr::null(), 0, 0, 0) == 0
                || WinHttpReceiveResponse(request.0, ptr::null_mut()) == 0 { return Err("network".into()); }
            let mut status: u32 = 0;
            let mut size: u32 = 4;
            if WinHttpQueryHeaders(request.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, ptr::null(), &mut status as *mut _ as _, &mut size, ptr::null_mut()) == 0 { return Err("network".into()); }
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                let mut location = vec![0u16; 8192];
                let mut size = (location.len() * 2) as u32;
                if WinHttpQueryHeaders(request.0, WINHTTP_QUERY_LOCATION, ptr::null(), location.as_mut_ptr() as _, &mut size, ptr::null_mut()) == 0 { return Err("redirect".into()); }
                let next = String::from_utf16_lossy(&location[..location.iter().position(|v| *v == 0).unwrap_or(location.len())]);
                url = allowed_url(url.join(&next).map_err(|_| "redirect")?.as_str())?;
                continue;
            }
            if status != 200 { return Ok(status as u16); }
            let mut buffer = [0u8; 65536];
            let mut total = 0u64;
            loop {
                if started.elapsed() > Duration::from_secs(600) { return Err("timeout".into()); }
                let mut count = 0u32;
                if WinHttpReadData(request.0, buffer.as_mut_ptr() as _, buffer.len() as u32, &mut count) == 0 { return Err("network".into()); }
                if count == 0 { break; }
                total += u64::from(count);
                if total > limit { return Err("size".into()); }
                receive(&buffer[..count as usize])?;
            }
            return Ok(200);
        }
    }
    Err("redirect".into())
}

pub fn bytes(url: &str, limit: u64) -> Result<(u16, Vec<u8>), String> {
    let mut out = Vec::new();
    let status = get(url, limit, |chunk| { out.extend_from_slice(chunk); Ok(()) })?;
    Ok((status, out))
}
