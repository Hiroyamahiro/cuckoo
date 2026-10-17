use tauri::{Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
pub const LABEL: &str = "tray-panel";
const WIDTH: f64 = 196.0;
const HEIGHT: f64 = 184.0;
fn place(x: f64, y: f64, left: f64, top: f64, width: f64, height: f64, scale: f64) -> (i32, i32) {
    let w = WIDTH * scale;
    let h = HEIGHT * scale;
    (x.clamp(left, (left + width - w).max(left)) as i32,
     (y - h).clamp(top, (top + height - h).max(top)) as i32)
}
pub fn open(app: &tauri::AppHandle, cursor: PhysicalPosition<f64>) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window(LABEL) {
            let _ = window.destroy();
            return;
        }
        let result = (|| -> tauri::Result<()> {
            let monitor = handle.monitor_from_point(cursor.x, cursor.y)?;
            let window = WebviewWindowBuilder::new(&handle, LABEL, WebviewUrl::App("tray-panel.html".into()))
                .title("Cuckoo").inner_size(WIDTH, HEIGHT).resizable(false)
                .decorations(false).skip_taskbar(true).always_on_top(true)
                .shadow(true).visible(false).focused(false).build()?;
            if let Some(monitor) = monitor {
                let area = monitor.work_area();
                let (x,y) = place(cursor.x, cursor.y, area.position.x as f64, area.position.y as f64,
                                  area.size.width as f64, area.size.height as f64, monitor.scale_factor());
                window.set_position(PhysicalPosition::new(x,y))?;
            }
            // Panel windows never participate in main-window lifecycle/state restoration.
            let weak = handle.clone();
            window.on_window_event(move |event| {
                if matches!(event, tauri::WindowEvent::Focused(false) | tauri::WindowEvent::CloseRequested { .. }) {
                    if let Some(w) = weak.get_webview_window(LABEL) { let _ = w.destroy(); }
                }
            });
            window.show()?;
            window.set_focus()?;
            Ok(())
        })();
        if let Err(e) = result {
            if let Some(w) = handle.get_webview_window(LABEL) { let _ = w.destroy(); }
            eprintln!("tray panel: {e}");
        }
    });
}
#[tauri::command]
pub fn tray_panel_action(app: tauri::AppHandle, window: WebviewWindow, action: String) -> Result<(), String> {
    if window.label() != LABEL { return Err("Invalid tray panel".into()); }
    match action.as_str() {
        "toggle" => {
            let state = app.state::<std::sync::Arc<super::State>>();
            state.settings.toggle()?;
            super::changed(&app, &state);
            // Deliberately keep the panel open until blur/Escape/another action.
        }
        "open" => { window.destroy().map_err(|e| e.to_string())?; super::show(&app); }
        "quit" => { window.destroy().map_err(|e| e.to_string())?; super::shutdown(&app); }
        "close" => { window.destroy().map_err(|e| e.to_string())?; }
        _ => return Err("Invalid tray action".into()),
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn panel_position_stays_in_negative_and_high_dpi_work_areas() {
        assert_eq!(super::place(1910.,1040.,0.,0.,1920.,1040.,1.),(1724,856));
        assert_eq!(super::place(-1900.,10.,-1920.,0.,1920.,1080.,1.5),(-1900,0));
        assert_eq!(super::place(0.,0.,0.,0.,100.,80.,2.),(0,0));
    }
}
