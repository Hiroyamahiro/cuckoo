//! UI-thread resize timer and interruption handling for the HTML titlebar.
use tauri::{Manager, WebviewWindow};
use windows_sys::Win32::{
    Foundation::*,
    UI::{Shell::*, WindowsAndMessaging::*},
};
pub const RESIZE_TIMER: usize = 0x4355434b;
const SUBCLASS: usize = 0x43554652;
struct Context {
    app: tauri::AppHandle,
    window: WebviewWindow,
}

pub fn install(app: &tauri::AppHandle, w: &WebviewWindow) -> Result<(), String> {
    let hwnd = w.hwnd().map_err(|e| e.to_string())?.0 as HWND;
    let context = Box::into_raw(Box::new(Context {
        app: app.clone(),
        window: w.clone(),
    }));
    unsafe {
        if SetWindowSubclass(hwnd, Some(procedure), SUBCLASS, context as usize) == 0 {
            drop(Box::from_raw(context));
            return Err("ウィンドウの描画処理を初期化できません。".into());
        }
    }
    Ok(())
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    data: usize,
) -> LRESULT {
    // No unwind may cross the Win32 callback boundary. The default procedure
    // still receives every native caption, snap and accessibility message.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let c = &*(data as *const Context);
        match msg {
            WM_TIMER if wp == RESIZE_TIMER => super::native_display::tick(&c.app, &c.window),
            WM_ENTERSIZEMOVE => {
                let _ = super::native_display::settle(&c.app, &c.window);
            }
            WM_SYSCOMMAND
                if matches!(wp as u32 & 0xfff0, SC_MAXIMIZE | SC_RESTORE | SC_MINIMIZE) =>
            {
                let _ = super::native_display::settle(&c.app, &c.window);
            }
            WM_SETTINGCHANGE => {
                if !super::native_display::motion_enabled() {
                    let _ = super::native_display::settle(&c.app, &c.window);
                }
            }
            WM_NCDESTROY => {
                KillTimer(hwnd, RESIZE_TIMER);
                let state = c.app.state::<std::sync::Arc<super::State>>();
                state.display.lock().unwrap().detach(c.window.label());
            }
            _ => (),
        }
    }));
    let result = DefSubclassProc(hwnd, msg, wp, lp);
    if msg == WM_NCDESTROY {
        RemoveWindowSubclass(hwnd, Some(procedure), SUBCLASS);
        drop(Box::from_raw(data as *mut Context));
    }
    result
}
