//! Geometry belongs to Rust, survives WebView destruction, and is not persisted.
//! All native calls run on the UI thread, outside the controller's mutex.
use std::time::{Duration, Instant};
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
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plan {
    pub rect: Rect,
    pub min_width: u32,
    pub min_height: u32,
    pub maximize: bool,
}
const TRANSITION: Duration = Duration::from_millis(220);
#[derive(Clone)]
struct Animation {
    from: Rect,
    plan: Plan,
    started: Instant,
    minimum: bool,
    scale: f64,
}
impl Animation {
    fn sample(&self, elapsed: Duration, origin: Rect) -> Rect {
        let t = (elapsed.as_secs_f64() / TRANSITION.as_secs_f64()).clamp(0., 1.);
        let ease = (1. - (std::f64::consts::PI * t).cos()) / 2.;
        let size =
            |a: u32, b: u32| (a as f64 + (b as f64 - a as f64) * ease).round().max(1.) as u32;
        Rect {
            width: size(self.from.width, self.plan.rect.width),
            height: size(self.from.height, self.plan.rect.height),
            ..origin
        }
    }
}
#[derive(Default, Clone)]
pub struct Display {
    pub label: Option<String>,
    pub minimum: bool,
    pub normal: Option<Snapshot>,
    pub current: Option<Snapshot>,
    animation: Option<Animation>,
    minimum_size: (f64, f64),
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
        self.animation = None;
        self.label = Some(label.into());
        let initial = Snapshot {
            rect: o.rect,
            maximized: false,
        };
        let s = self.current.unwrap_or(initial);
        if self.normal.is_none() {
            self.normal = Some(initial);
        }
        self.current = Some(s);
        Plan {
            rect: s.rect,
            min_width: px(self.minimum_size.0, o.scale)
                .saturating_add(o.frame.width)
                .min(s.rect.width),
            min_height: if self.minimum {
                s.rect.height
            } else {
                px(self.minimum_size.1, o.scale)
                    .saturating_add(o.frame.height)
                    .min(s.rect.height)
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
            // A reversal must not replace the saved normal geometry with an
            // intermediate frame from the previous transition.
            if self.animation.is_none() {
                self.normal = Some(Snapshot {
                    rect: if o.maximized { o.normal_rect } else { o.rect },
                    maximized: o.maximized,
                });
            }
            Some(self.minimum_plan(o))
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
                min_width: px(self.minimum_size.0, o.scale)
                    .saturating_add(o.frame.width)
                    .min(saved.rect.width),
                min_height: px(self.minimum_size.1, o.scale)
                    .saturating_add(o.frame.height)
                    .min(saved.rect.height),
                maximize: saved.maximized,
            })
        }
    }
    pub fn fit(&self, label: &str, o: Observed) -> Result<Option<Plan>, String> {
        if self.label.as_deref() != Some(label) {
            return Err("古いウィンドウからの要求です。".into());
        }
        if !self.minimum {
            return Ok(None);
        }
        Ok(Some(self.minimum_plan(o)))
    }
    fn minimum_plan(&self, o: Observed) -> Plan {
        let width = px(self.minimum_size.0, o.scale).saturating_add(o.frame.width);
        let height = px(self.minimum_size.1, o.scale).saturating_add(o.frame.height);
        Plan {
            rect: Rect {
                width,
                height,
                ..if o.maximized { o.normal_rect } else { o.rect }
            },
            min_width: width,
            min_height: height,
            maximize: false,
        }
    }

    pub fn observe(&mut self, label: &str, o: Observed) {
        if self.label.as_deref() != Some(label) || o.minimized || self.animation.is_some() {
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
    pub fn detach(&mut self, label: &str) {
        if self.label.as_deref() == Some(label) {
            self.animation = None;
        }
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
    let rect = bounded(w, plan.rect, true)?;
    apply_rect(w, plan, rect)
}
fn bounded(w: &WebviewWindow, rect: Rect, reposition: bool) -> Result<Rect, String> {
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
    let area = best_area(rect, &areas);
    // Attaching a recreated window may recover an off-screen position. Mode
    // switches preserve the user's top-left, including near a work-area edge.
    Ok(if reposition {
        within(rect, area)
    } else {
        Rect {
            width: rect.width.min(area.width),
            height: rect.height.min(area.height),
            ..rect
        }
    })
}
fn apply_rect(w: &WebviewWindow, plan: Plan, rect: Rect) -> Result<(), String> {
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
pub fn motion_enabled() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let mut client: i32 = 0;
    let mut animation = ANIMATIONINFO {
        cbSize: std::mem::size_of::<ANIMATIONINFO>() as u32,
        iMinAnimate: 0,
    };
    unsafe {
        SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, &mut client as *mut _ as _, 0) != 0
            && client != 0
            && SystemParametersInfoW(
                SPI_GETANIMATION,
                animation.cbSize,
                &mut animation as *mut _ as _,
                0,
            ) != 0
            && animation.iMinAnimate != 0
    }
}
fn kill_timer(w: &WebviewWindow) {
    if let Ok(hwnd) = w.hwnd() {
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::KillTimer(
                hwnd.0 as _,
                super::native_frame::RESIZE_TIMER,
            );
        }
    }
}
fn animate(app: &tauri::AppHandle, w: &WebviewWindow, mut plan: Plan) -> Result<(), String> {
    let state = app.state::<std::sync::Arc<crate::State>>();
    let o = observed(w)?;
    plan.rect = bounded(w, plan.rect, false)?;
    {
        let d = state.display.lock().unwrap();
        if d.animation
            .as_ref()
            .is_some_and(|a| a.minimum == d.minimum && a.plan == plan)
        {
            return Ok(());
        }
    }
    kill_timer(w);
    if !motion_enabled()
        || !w.is_visible().unwrap_or(false)
        || o.maximized
        || o.minimized
        || (o.rect.width == plan.rect.width && o.rect.height == plan.rect.height)
    {
        state.display.lock().unwrap().animation = None;
        apply_rect(w, plan, plan.rect)?;
        let o = observed(w)?;
        state.display.lock().unwrap().observe(w.label(), o);
        return Ok(());
    }
    w.set_min_size(Some(PhysicalSize::new(1, 1)))
        .map_err(|e| e.to_string())?;
    {
        let mut d = state.display.lock().unwrap();
        let started = d
            .animation
            .as_ref()
            .filter(|a| a.minimum == d.minimum)
            .map(|a| a.started)
            .unwrap_or_else(Instant::now);
        // Retarget a content-height refit without stretching the 220ms deadline.
        let from = d
            .animation
            .as_ref()
            .filter(|a| a.minimum == d.minimum)
            .map(|a| a.from)
            .unwrap_or(o.rect);
        d.animation = Some(Animation {
            from,
            plan,
            started,
            minimum: d.minimum,
            scale: o.scale,
        });
    }
    let hwnd = w.hwnd().map_err(|e| e.to_string())?;
    if unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SetTimer(
            hwnd.0 as _,
            super::native_frame::RESIZE_TIMER,
            15,
            None,
        )
    } == 0
    {
        return settle(app, w);
    }
    Ok(())
}
pub fn settle(app: &tauri::AppHandle, w: &WebviewWindow) -> Result<(), String> {
    kill_timer(w);
    let state = app.state::<std::sync::Arc<crate::State>>();
    let animation = {
        let mut d = state.display.lock().unwrap();
        if d.label.as_deref() != Some(w.label()) {
            return Ok(());
        }
        d.animation.take()
    };
    if let Some(mut a) = animation {
        let o = observed(w)?;
        a.plan.rect.x = o.rect.x;
        a.plan.rect.y = o.rect.y;
        apply_rect(w, a.plan, a.plan.rect)?;
        let o = observed(w)?;
        state.display.lock().unwrap().observe(w.label(), o);
    }
    Ok(())
}
pub fn tick(app: &tauri::AppHandle, w: &WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let state = app.state::<std::sync::Arc<crate::State>>();
    let animation = {
        let d = state.display.lock().unwrap();
        if d.label.as_deref() != Some(w.label()) {
            return;
        }
        d.animation.clone()
    };
    let Some(a) = animation else {
        kill_timer(w);
        return;
    };
    let result = (|| -> Result<(), String> {
        let o = observed(w)?;
        if o.maximized || o.minimized || o.scale != a.scale {
            kill_timer(w);
            let mut d = state.display.lock().unwrap();
            d.animation = None;
            d.observe(w.label(), o);
            return Ok(());
        }
        if a.started.elapsed() >= TRANSITION || !motion_enabled() {
            return settle(app, w);
        }
        let r = a.sample(a.started.elapsed(), o.rect);
        let hwnd = w.hwnd().map_err(|e| e.to_string())?;
        if unsafe {
            SetWindowPos(
                hwnd.0 as _,
                std::ptr::null_mut(),
                0,
                0,
                r.width as i32,
                r.height as i32,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        } == 0
        {
            return Err("ウィンドウ寸法を変更できません。".into());
        }
        Ok(())
    })();
    if let Err(error) = result {
        kill_timer(w);
        state.display.lock().unwrap().animation = None;
        *state.display_error.lock().unwrap() = error;
    }
}
pub fn attach(app: &tauri::AppHandle, w: &WebviewWindow) -> Result<(), String> {
    let state = app.state::<std::sync::Arc<crate::State>>();
    let o = observed(w)?;
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|c| c.label == "main")
        .ok_or("Main window configuration missing")?;
    let plan = {
        let mut d = state.display.lock().unwrap();
        d.minimum_size = (
            config.min_width.ok_or("minWidth missing")?,
            config.min_height.ok_or("minHeight missing")?,
        );
        d.attach(w.label(), o)
    };
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
        let result = animate(app, w, p);
        if let Err(e) = result {
            *state.display.lock().unwrap() = before;
            return Err(e);
        }
    }
    Ok(())
}
pub fn fit(app: &tauri::AppHandle, w: &WebviewWindow) -> Result<(), String> {
    sync(app, w)?;
    let state = app.state::<std::sync::Arc<crate::State>>();
    let o = observed(w)?;
    let plan = state.display.lock().unwrap().fit(w.label(), o)?;
    if let Some(p) = plan {
        animate(app, w, p)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn display() -> Display {
        Display {
            minimum_size: (400., 400.),
            ..Display::default()
        }
    }
    #[test]
    fn initial_size_is_preserved_and_minimum_uses_configured_dimensions() {
        let mut d = display();
        d.minimum_size = (480., 420.);
        let mut o = observed();
        o.rect.width = 500;
        assert_eq!(d.attach("w", o).rect, o.rect);
        d.mode("w", true, o);
        let a = d.fit("w", o).unwrap().unwrap();
        o.rect.width = 800;
        o.rect.height = 1100;
        let b = d.fit("w", o).unwrap().unwrap();
        assert_eq!(a, b);
        assert_eq!((a.rect.width, a.rect.height), (496, 459));
        o.scale = 1.5;
        let p = d.fit("w", o).unwrap().unwrap();
        assert_eq!((p.rect.width, p.rect.height), (736, 669));
    }
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
    fn resize_eases_for_220ms_without_changing_the_current_origin() {
        let o = observed();
        let target = Plan {
            rect: Rect {
                height: 500,
                ..o.rect
            },
            min_width: 400,
            min_height: 500,
            maximize: false,
        };
        let a = Animation {
            from: o.rect,
            plan: target,
            started: Instant::now(),
            minimum: true,
            scale: 1.,
        };
        assert_eq!(a.sample(Duration::ZERO, o.rect), o.rect);
        assert_eq!(a.sample(Duration::from_millis(110), o.rect).height, 700);
        assert_eq!(a.sample(TRANSITION, o.rect).height, 500);
        let moved = Rect {
            x: -600,
            y: 80,
            ..o.rect
        };
        assert_eq!(
            (
                a.sample(Duration::from_millis(150), moved).x,
                a.sample(Duration::from_millis(150), moved).y
            ),
            (-600, 80)
        );
        let mut last = 900;
        for ms in 0..=220 {
            let height = a.sample(Duration::from_millis(ms), o.rect).height;
            assert!(height <= last);
            last = height;
        }
    }
    #[test]
    fn repeated_reversals_and_transient_observations_keep_the_normal_snapshot() {
        let mut d = display();
        let mut o = observed();
        d.attach("main-1", o);
        d.mode("main-1", true, o);
        let p = d.fit("main-1", o).unwrap().unwrap();
        d.animation = Some(Animation {
            from: o.rect,
            plan: p,
            started: Instant::now(),
            minimum: true,
            scale: 1.,
        });
        o.rect.height = 750;
        d.observe("main-1", o);
        assert_ne!(d.current.unwrap().rect.height, 750);
        assert_eq!(d.mode("main-1", false, o).unwrap().rect.height, 900);
        d.mode("main-1", true, o);
        assert_eq!(d.normal.unwrap().rect.height, 900);
        d.detach("old-window");
        assert!(d.animation.is_some());
        d.detach("main-1");
        assert!(d.animation.is_none());
        assert_eq!(d.mode("main-1", false, o).unwrap().rect.height, 900);
    }
    #[test]
    fn modes_preserve_normal_and_survive_recreation() {
        let mut d = display();
        let mut o = observed();
        d.attach("main-1", o);
        d.observe("main-1", o);
        let p = d.mode("main-1", true, o).unwrap();
        assert_eq!((p.rect.width, p.rect.height), (416, 439));
        let p = d.fit("main-1", o).unwrap().unwrap();
        assert_eq!(p.rect.height, 439);
        o.rect = p.rect;
        d.observe("main-1", o);
        let restored = d.attach("main-2", o);
        assert_eq!(restored.rect.height, 439);
        assert!(d.fit("main-1", o).is_err());
        let normal = d.mode("main-2", false, o).unwrap();
        assert_eq!(normal.rect.height, 900);
        assert_eq!(normal.rect.x, 200);
    }
    #[test]
    fn returning_to_normal_keeps_the_moved_minimum_origin() {
        let mut d = display();
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
    fn maximized_restores_and_dpi_converts_configured_dimensions() {
        let mut d = display();
        let mut o = observed();
        d.attach("w", o);
        d.observe("w", o);
        o.maximized = true;
        o.rect.width = 1920;
        d.mode("w", true, o);
        o.scale = 1.5;
        o.frame.height = 58;
        let p = d.fit("w", o).unwrap().unwrap();
        assert_eq!(p.rect.height, 658);
        assert!(d.mode("w", false, o).unwrap().maximize);
    }
    #[test]
    fn work_area_clamp() {
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
        let mut d = display();
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
        let p = d.mode("second", true, o).unwrap();
        assert_eq!((p.rect.x, p.rect.y), (o.normal_rect.x, o.normal_rect.y));
        assert_eq!((p.rect.width, p.rect.height), (416, 439));
    }
}
