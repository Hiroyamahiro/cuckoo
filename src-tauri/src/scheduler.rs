use chrono::{DateTime, Local, TimeZone, Timelike};
use serde_json::Value;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
pub const TICK: Duration = Duration::from_millis(250);
// Wall/monotonic mismatch beyond 500ms exceeds normal sample skew.
const TOLERANCE: i64 = 500;
pub fn repeat_for(minute: u32, s: &Value) -> u64 {
    if minute == 0 {
        s["hourlyRepeat"].as_u64().unwrap_or(2)
    } else {
        s["halfHourlyRepeat"].as_u64().unwrap_or(1)
    }
}
pub fn next(now: DateTime<Local>, s: &Value) -> Option<i64> {
    if s["enabled"] != true {
        return None;
    }
    for day in 0..=8 {
        let date = now.date_naive() + chrono::Duration::days(day);
        for i in 0..48 {
            if s["slots"][i] != true {
                continue;
            }
            let naive = date.and_hms_opt((i / 2) as u32, if i % 2 == 0 { 0 } else { 30 }, 0)?;
            if let Some(t) = Local.from_local_datetime(&naive).earliest() {
                if t > now {
                    return Some(t.timestamp_millis());
                }
            }
        }
    }
    None
}
#[derive(Default)]
pub struct Scheduler {
    wall: Option<i64>,
    mono: Option<Instant>,
    history: VecDeque<(String, Instant)>,
    revision: Value,
}
impl Scheduler {
    pub fn poll(&mut self, now: DateTime<Local>, mono: Instant, s: &Value) -> (bool, bool) {
        let wall = now.timestamp_millis();
        let elapsed = self.mono.map(|x| mono.saturating_duration_since(x));
        let jumped = self
            .wall
            .zip(elapsed)
            .map(|(last, e)| {
                wall - last < 0
                    || e > Duration::from_millis(2500)
                    || ((wall - last) - e.as_millis() as i64).abs() > TOLERANCE
            })
            .unwrap_or(true);
        let previous = self.wall.replace(wall);
        self.mono = Some(mono);
        let revision = serde_json::json!([s["enabled"], s["slots"]]);
        let changed = self.revision != revision;
        self.revision = revision;
        self.history
            .retain(|(_, t)| mono.saturating_duration_since(*t) <= Duration::from_secs(7 * 86400));
        if jumped || changed || s["enabled"] != true {
            return (false, jumped);
        }
        let boundary = wall - (now.second() as i64 * 1000 + now.timestamp_subsec_millis() as i64);
        let slot = (now.hour() * 2 + now.minute() / 30) as usize;
        if now.minute() % 30 != 0
            || wall - boundary > 2000
            || previous.unwrap_or(wall) >= boundary
            || s["slots"][slot] != true
        {
            return (false, false);
        }
        let key = now.format("%Y-%m-%d-%H-%M").to_string();
        if self.history.iter().any(|(k, _)| *k == key) {
            return (false, false);
        }
        self.history.push_back((key, mono));
        while self.history.len() > 384 {
            self.history.pop_front();
        }
        (true, false)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn time(h: u32, m: u32, s: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 10, 16, h, m, s)
            .single()
            .unwrap()
    }
    fn settings() -> Value {
        json!({"enabled":true,"slots":vec![true;48]})
    }
    #[test]
    fn future_only() {
        assert_eq!(
            next(time(9, 0, 0), &settings()),
            Some(time(9, 30, 0).timestamp_millis())
        );
    }
    #[test]
    fn disabled() {
        assert_eq!(next(time(9, 0, 0), &json!({"enabled":false})), None);
    }
    #[test]
    fn once() {
        let mut c = Scheduler::default();
        let t = Instant::now();
        let s = settings();
        assert!(!c.poll(time(8, 59, 59), t, &s).0);
        assert!(c.poll(time(9, 0, 0), t + Duration::from_secs(1), &s).0);
        assert!(!c.poll(time(9, 0, 0), t + Duration::from_millis(1250), &s).0);
    }
    #[test]
    fn sleep_skip() {
        let mut c = Scheduler::default();
        let t = Instant::now();
        let s = settings();
        c.poll(time(8, 59, 59), t, &s);
        assert!(!c.poll(time(9, 0, 1), t + Duration::from_secs(10), &s).0);
    }
    #[test]
    fn correction_skip() {
        let mut c = Scheduler::default();
        let t = Instant::now();
        let s = settings();
        c.poll(time(8, 59, 59), t, &s);
        assert!(!c.poll(time(9, 0, 0), t + Duration::from_millis(250), &s).0);
    }
    #[test]
    fn zero_and_half_hour_continue_without_any_ui() {
        let mut c = Scheduler::default();
        let t = Instant::now();
        let s = settings();
        c.poll(time(8, 59, 59), t, &s);
        assert!(c.poll(time(9, 0, 0), t + Duration::from_secs(1), &s).0);
        c.poll(time(9, 29, 59), t + Duration::from_secs(1800), &s);
        assert!(c.poll(time(9, 30, 0), t + Duration::from_secs(1801), &s).0);
        assert!(
            !c.poll(time(9, 30, 0), t + Duration::from_millis(1801250), &s)
                .0
        );
        c.poll(time(9, 59, 59), t + Duration::from_secs(3600), &s);
        assert!(c.poll(time(10, 0, 0), t + Duration::from_secs(3601), &s).0);
    }
    #[test]
    fn repeat_counts_are_independent_of_ui_lifetime() {
        let independent=serde_json::json!({"hourlyRepeat":1,"halfHourlyRepeat":2});
        assert_eq!(repeat_for(0,&independent),1);
        assert_eq!(repeat_for(30,&independent),2);
        let s = serde_json::json!({"hourlyRepeat":2});
        assert_eq!(repeat_for(0, &s), 2);
        assert_eq!(repeat_for(30, &s), 1);
        assert_eq!(repeat_for(0, &serde_json::json!({"hourlyRepeat":1})), 1);
    }
    #[test]
    fn minimum_display_changes_do_not_drop_scheduled_boundaries() {
        let mut c = Scheduler::default();
        let t = Instant::now();
        let mut s = settings();
        s["minimumDisplay"] = json!(false);
        c.poll(time(8, 59, 59), t, &s);
        s["minimumDisplay"] = json!(true);
        assert!(c.poll(time(9, 0, 0), t + Duration::from_secs(1), &s).0);
        c.poll(time(9, 29, 59), t + Duration::from_secs(1800), &s);
        s["minimumDisplay"] = json!(false);
        assert!(c.poll(time(9, 30, 0), t + Duration::from_secs(1801), &s).0);
    }
}
