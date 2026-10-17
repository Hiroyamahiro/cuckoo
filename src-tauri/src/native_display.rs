//! Geometry belongs to Rust, survives WebView destruction, and is not persisted.
//! All native calls run on the UI thread, outside the controller's mutex.
use tauri::{Manager, PhysicalPosition, PhysicalSize, WebviewWindow};
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Copy, Debug)]
pub struct Snapshot {
    pub rect: Rect,
    pub maximized: bool,
}
#[derive(Clone, Copy)]
pub struct Observed {
    pub rect: Rect,
    pub normal_rect: Rect,
    pub frame: PhysicalSize<u32>,
    pub scale: f64,
    pub maximized: bool,
    pub minimized: bool,
}
#[derive(Clone, Copy)]
pub struct Plan {
    pub rect: Rect,
    pub min_width: u32,
    pub min_height: u32,
    pub maximize: bool,
}
#[derive(Default, Clone)]
pub struct Display {
    pub label: Option<String>,
    pub minimum: bool,
    pub normal: Option<Snapshot>,
    pub current: Option<Snapshot>,
}
pub fn valid_height(h: f64) -> Result<(), String> {
    if h.is_finite() && (300.0..=1600.0).contains(&h) {
        Ok(())
    } else {
        Err("表示領域の高さが不正です。".into())
    }
}
fn px(n: f64, scale: f64) -> u32 {
    (n * scale).round().clamp(1.0, u32::MAX as f64) as u32
}
pub fn within(r: Rect, a: Rect) -> Rect {
    let width = r.width.min(a.width).max(1);
    let height = r.height.min(a.height).max(1);
    Rect {
        width,
        height,
        x: (r.x as i64).clamp(a.x as i64, a.x as i64 + a.width as i64 - width as i64) as i32,
        y: (r.y as i64).clamp(a.y as i64, a.y as i64 + a.height as i64 - height as i64) as i32,
    }
}
pub fn best_area(r: Rect, areas: &[Rect]) -> Rect {
    *areas
        .iter()
        .max_by_key(|a| {
            let w = (r.x as i64 + r.width as i64).min(a.x as i64 + a.width as i64)
                - (r.x.max(a.x) as i64);
            let h = (r.y as i64 + r.height as i64).min(a.y as i64 + a.height as i64)
                - (r.y.max(a.y) as i64);
            (
                w.max(0) * h.max(0),
                -((r.x as i64 - a.x as i64).abs() + (r.y as i64 - a.y as i64).abs()),
            )
        })
        .expect("at least one monitor")
}
impl Display {
    pub fn attach(&mut self, label: &str, o: Observed) -> Plan {
        self.label = Some(label.into());
        let initial = Snapshot {
            rect: Rect {
                width: px(550., o.scale),
                height: px(1000., o.scale),
                ..o.rect
            },
            maximized: false,
        };
        let s = self.current.unwrap_or(initial);
        if self.normal.is_none() {
            self.normal = Some(initial);
        }
        self.current = Some(s);
        Plan {
            rect: s.rect,
            min_width: px(400., o.scale).min(s.rect.width),
            min_height: if self.minimum {
                s.rect.height
            } else {
                px(400., o.scale).min(s.rect.height)
            },
            maximize: s.maximized && !self.minimum,
        }
    }
    pub fn mode(&mut self, label: &str, minimum: bool, o: Observed) -> Option<Plan> {
        if self.label.as_deref() != Some(label) || self.minimum == minimum {
            return None;
        }
        self.minimum = minimum;
        if minimum {
            let normal = if o.maximized { o.normal_rect } else { o.rect };
            self.normal = Some(Snapshot {
                rect: normal,
                maximized: o.maximized,
            });
            Some(Plan {
                rect: normal,
                min_width: px(400., o.scale).min(normal.width),
                min_height: px(300., o.scale).min(normal.height),
                maximize: false,
            })
        } else {
            let saved = self.normal.unwrap_or(Snapshot {
                rect: o.rect,
                maximized: false,
            });
            Some(Plan {
                rect: Rect {
                    x: o.rect.x,
                    y: o.rect.y,
                    ..saved.rect
                },
                min_width: px(600., o.scale).min(saved.rect.width),
                min_height: px(400., o.scale).min(saved.rect.height),
                maximize: saved.maximized,
            })
        }
    }
    pub fn fit(&self, label: &str, h: f64, o: Observed) -> Result<Option<Plan>, String> {
        valid_height(h)?;
        if self.label.as_deref() != Some(label) {
            return Err("古いウィンドウからの要求です。".into());
        }
        if !self.minimum {
            return Ok(None);
        }
        let height = px(h.ceil(), o.scale).saturating_add(o.frame.height);
        Ok(Some(Plan {
            rect: Rect { height, ..o.rect },
            min_width: px(600., o.scale).min(o.rect.width),
            min_height: height,
            maximize: false,
        }))
    }
    pub fn observe(&mut self, label: &str, o: Observed) {
        if self.label.as_deref() != Some(label) || o.minimized {
            return;
        }
        self.current = Some(Snapshot {
            rect: if o.maximized && !self.minimum {
                o.normal_rect
            } else {
                o.rect
            },
            maximized: o.maximized,
        });
    }
}
pub fn observed(w: &WebviewWindow) -> Result<Observed, String> {
    let size = w.outer_size().map_err(|e| e.to_string())?;
    let inner = w.inner_size().map_err(|e| e.to_string())?;
    let p = w.outer_position().map_err(|e| e.to_string())?;
    let rect = Rect {
        x: p.x,
        y: p.y,
        width: size.width,
        height: size.height,
    };
    let maximized = w.is_maximized().map_err(|e| e.to_string())?;
    let normal_rect = if maximized {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowPlacement, WINDOWPLACEMENT};
        let mut placement: WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
        placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
        if unsafe {
            GetWindowPlacement(w.hwnd().map_err(|e| e.to_string())?.0 as _, &mut placement)
        } == 0
        {
            return Err("通常ウィンドウ位置を取得できません。".into());
        }
        let r = placement.rcNormalPosition;
        let monitor = w
            .current_monitor()
            .map_err(|e| e.to_string())?
            .ok_or("モニターを取得できません。")?;
        // WINDOWPLACEMENT is in workspace coordinates, not screen coordinates.
        let work = monitor.work_area();
        let origin = monitor.position();
        Rect {
            x: r.left + work.position.x - origin.x,
            y: r.top + work.position.y - origin.y,
            width: (r.right - r.left).max(1) as u32,
            height: (r.bottom - r.top).max(1) as u32,
        }
    } else {
        rect
    };
    Ok(Observed {
        rect,
        normal_rect,
        frame: PhysicalSize::new(
            size.width.saturating_sub(inner.width),
            size.height.saturating_sub(inner.height),
        ),
        scale: w.scale_factor().map_err(|e| e.to_string())?,
        maximized,
        minimized: w.is_minimized().map_err(|e| e.to_string())?,
    })
}
pub fn apply(w: &WebviewWindow, plan: Plan) -> Result<(), String> {
    let monitors = w.available_monitors().map_err(|e| e.to_string())?;
    let areas = monitors
        .iter()
        .map(|m| {
            let a = m.work_area();
            Rect {
                x: a.position.x,
                y: a.position.y,
                width: a.size.width,
                height: a.size.height,
            }
        })
        .filter(|a| a.width > 0 && a.height > 0)
        .collect::<Vec<_>>();
    if areas.is_empty() {
        return Err("作業領域を取得できません。".into());
    }
    let rect = within(plan.rect, best_area(plan.rect, &areas));
    if w.is_maximized().map_err(|e| e.to_string())? {
        w.unmaximize().map_err(|e| e.to_string())?;
    }
    let o = observed(w)?;
    // Release constraints before shrinking; Tauri sizes are INNER physical pixels.
    w.set_min_size(Some(PhysicalSize::new(1, 1)))
        .map_err(|e| e.to_string())?;
    w.set_size(PhysicalSize::new(
        rect.width.saturating_sub(o.frame.width).max(1),
        rect.height.saturating_sub(o.frame.height).max(1),
    ))
    .map_err(|e| e.to_string())?;
    w.set_position(PhysicalPosition::new(rect.x, rect.y))
        .map_err(|e| e.to_string())?;
    w.set_min_size(Some(PhysicalSize::new(
        plan.min_width
            .min(rect.width)
            .saturating_sub(o.frame.width)
            .max(1),
        plan.min_height
            .min(rect.height)
            .saturating_sub(o.frame.height)
            .max(1),
    )))
    .map_err(|e| e.to_string())?;
    if plan.maximize {
        w.maximize().map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub fn attach(app: &tauri::AppHandle, w: &WebviewWindow) -> Result<(), String> {
    let state = app.state::<std::sync::Arc<crate::State>>();
    let o = observed(w)?;
    let plan = state.display.lock().unwrap().attach(w.label(), o);
    apply(w, plan)?;
    sync(app, w)
}
pub fn sync(app: &tauri::AppHandle, w: &WebviewWindow) -> Result<(), String> {
    let state = app.state::<std::sync::Arc<crate::State>>();
    let minimum = state.settings.snapshot()["minimumDisplay"] == true;
    let o = observed(w)?;
    let (before, plan) = {
        let mut d = state.display.lock().unwrap();
        let before = d.clone();
        (before, d.mode(w.label(), minimum, o))
    };
    if let Some(p) = plan {
        if let Err(e) = apply(w, p) {
            *state.display.lock().unwrap() = before;
            return Err(e);
        }
    }
    Ok(())
}
pub fn fit(app: &tauri::AppHandle, w: &WebviewWindow, height: f64) -> Result<(), String> {
    sync(app, w)?;
    let state = app.state::<std::sync::Arc<crate::State>>();
    let o = observed(w)?;
    let plan = state.display.lock().unwrap().fit(w.label(), height, o)?;
    if let Some(p) = plan {
        apply(w, p)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn observed() -> Observed {
        Observed {
            rect: Rect {
                x: 200,
                y: 120,
                width: 550,
                height: 900,
            },
            normal_rect: Rect {
                x: 200,
                y: 120,
                width: 550,
                height: 900,
            },
            frame: PhysicalSize::new(16, 39),
            scale: 1.,
            maximized: false,
            minimized: false,
        }
    }
    #[test]
    fn modes_preserve_normal_and_survive_recreation() {
        let mut d = Display::default();
        let mut o = observed();
        d.attach("main-1", o);
        d.observe("main-1", o);
        let p = d.mode("main-1", true, o).unwrap();
        assert_eq!(p.rect, o.rect);
        let p = d.fit("main-1", 600., o).unwrap().unwrap();
        assert_eq!(p.rect.height, 639);
        o.rect = p.rect;
        d.observe("main-1", o);
        let restored = d.attach("main-2", o);
        assert_eq!(restored.rect.height, 639);
        assert!(d.fit("main-1", 600., o).is_err());
        let normal = d.mode("main-2", false, o).unwrap();
        assert_eq!(normal.rect.height, 900);
        assert_eq!(normal.rect.x, 200);
    }
    #[test]
    fn returning_to_normal_keeps_the_moved_minimum_origin() {
        let mut d = Display::default();
        let mut o = observed();
        d.attach("w", o);
        d.mode("w", true, o);
        o.rect.x = 640;
        o.rect.y = 80;
        o.rect.height = 620;
        d.observe("w", o);
        let p = d.mode("w", false, o).unwrap();
        assert_eq!((p.rect.x, p.rect.y), (640, 80));
        assert_eq!((p.rect.width, p.rect.height), (550, 900));
    }
    #[test]
    fn maximized_restores_and_dpi_converts_content_height() {
        let mut d = Display::default();
        let mut o = observed();
        d.attach("w", o);
        d.observe("w", o);
        o.maximized = true;
        o.rect.width = 1920;
        d.mode("w", true, o);
        o.scale = 1.5;
        o.frame.height = 58;
        let p = d.fit("w", 600., o).unwrap().unwrap();
        assert_eq!(p.rect.height, 958);
        assert!(d.mode("w", false, o).unwrap().maximize);
    }
    #[test]
    fn height_validation_and_work_area_clamp() {
        for h in [f64::NAN, f64::INFINITY, -1., 299., 1601.] {
            assert!(valid_height(h).is_err());
        }
        let a = Rect {
            x: -1280,
            y: 0,
            width: 1280,
            height: 720,
        };
        let r = within(
            Rect {
                x: -2000,
                y: 900,
                width: 2000,
                height: 1000,
            },
            a,
        );
        assert_eq!(
            r,
            Rect {
                x: -1280,
                y: 0,
                width: 1280,
                height: 720
            }
        );
        assert_eq!(
            best_area(
                r,
                &[
                    a,
                    Rect {
                        x: 0,
                        y: 0,
                        width: 1920,
                        height: 1040
                    }
                ]
            ),
            a
        );
    }
    #[test]
    fn maximized_recreation_uses_native_normal_placement() {
        let mut d = Display::default();
        let mut o = observed();
        d.attach("first", o);
        o.maximized = true;
        o.rect = Rect {
            x: -8,
            y: -8,
            width: 2576,
            height: 1416,
        };
        d.observe("first", o);
        let recreated = d.attach("second", o);
        assert!(recreated.maximize);
        assert_eq!(recreated.rect, o.normal_rect);
        assert_eq!(d.mode("second", true, o).unwrap().rect, o.normal_rect);
    }
}
