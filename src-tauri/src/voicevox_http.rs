//! Local Engine API only. No proxy, redirects, executable discovery or model downloads.
use std::{
    ffi::c_void,
    ptr,
    time::{Duration, Instant},
};
use windows_sys::Win32::Networking::WinHttp::*;
struct Handle(*mut c_void);
impl Handle {
    fn new(p: *mut c_void) -> Result<Self, String> {
        if p.is_null() {
            Err("VOICEVOXに接続できません。VOICEVOXを起動し、127.0.0.1:50021で接続できることを確認してください。".into())
        } else {
            Ok(Self(p))
        }
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            WinHttpCloseHandle(self.0);
        }
    }
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
pub fn request(method: &str, route: &str, body: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let failure = || {
        "VOICEVOXとの通信に失敗しました。エンジンの起動状態を確認して再試行してください。"
            .to_string()
    };
    let started = Instant::now();
    unsafe {
        let session = Handle::new(WinHttpOpen(
            wide("Cuckoo VOICEVOX").as_ptr(),
            WINHTTP_ACCESS_TYPE_NO_PROXY,
            ptr::null(),
            ptr::null(),
            0,
        ))?;
        WinHttpSetTimeouts(session.0, 3000, 3000, 5000, 90000);
        let conn = Handle::new(WinHttpConnect(
            session.0,
            wide("127.0.0.1").as_ptr(),
            50021,
            0,
        ))?;
        let req = Handle::new(WinHttpOpenRequest(
            conn.0,
            wide(method).as_ptr(),
            wide(route).as_ptr(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
        ))?;
        let policy = WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
        if WinHttpSetOption(
            req.0,
            WINHTTP_OPTION_REDIRECT_POLICY,
            &policy as *const _ as _,
            4,
        ) == 0
        {
            return Err(failure());
        }
        let headers = wide("Content-Type: application/json\r\n");
        if WinHttpSendRequest(
            req.0,
            headers.as_ptr(),
            (headers.len() - 1) as u32,
            if body.is_empty() {
                ptr::null()
            } else {
                body.as_ptr() as _
            },
            body.len() as u32,
            body.len() as u32,
            0,
        ) == 0
            || WinHttpReceiveResponse(req.0, ptr::null_mut()) == 0
        {
            return Err(failure());
        }
        let mut status = 0u32;
        let mut size = 4;
        if WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            ptr::null(),
            &mut status as *mut _ as _,
            &mut size,
            ptr::null_mut(),
        ) == 0
        {
            return Err(failure());
        }
        if status != 200 {
            return Err(format!("VOICEVOXが生成を受け付けませんでした（HTTP {status}）。接続先・キャラクター・文章を確認してください。"));
        }
        let mut out = Vec::new();
        let mut buf = [0u8; 16384];
        loop {
            if started.elapsed() > Duration::from_secs(120) {
                return Err(
                    "音声生成がタイムアウトしました。文章を短くして再試行してください。".into(),
                );
            }
            let mut n = 0;
            if WinHttpReadData(req.0, buf.as_mut_ptr() as _, buf.len() as u32, &mut n) == 0 {
                return Err(failure());
            }
            if n == 0 {
                break;
            }
            if out.len() + n as usize > limit {
                return Err("生成結果が上限を超えました。文章を短くしてください。".into());
            }
            out.extend_from_slice(&buf[..n as usize]);
        }
        Ok(out)
    }
}
