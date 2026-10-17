//! UI lifetime is independent of backend lifetime. Labels identify completed lifetimes.
//! A builder error reuses its label on retry: do not accumulate failed context labels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Hide,
    Destroy,
}
impl Default for Mode {
    fn default() -> Self {
        Self::Destroy
    }
}
impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Hide => "hide",
            Self::Destroy => "destroy",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Open,
    Store,
    Test,
    Quit,
}
pub struct Options {
    pub mode: Option<Mode>,
    pub action: Action,
    pub fail_create: Vec<u64>,
    pub fail_after_build: Vec<u64>,
}
pub fn options(args: &[String]) -> Result<Options, String> {
    let mut o = Options {
        mode: None,
        action: Action::Open,
        fail_create: vec![],
        fail_after_build: vec![],
    };
    for a in args.iter().skip(1) {
        if !cfg!(any(test, feature = "settings-test-harness")) && a.starts_with("--t2b") {
            return Err("Verification options are unavailable in release builds".into());
        }
        if a == "--request-update-exit" {
            o.action = Action::Quit;
        } else if let Some(v) = a.strip_prefix("--t2b2-window-mode=") {
            o.mode = Some(match v {
                "hide" => Mode::Hide,
                "destroy" => Mode::Destroy,
                _ => return Err("Invalid window mode".into()),
            });
        } else if let Some(v) = a.strip_prefix("--t2b2-action=") {
            o.action = match v {
                "open" => Action::Open,
                "store" => Action::Store,
                "test" => Action::Test,
                "quit" => Action::Quit,
                _ => return Err("Invalid action".into()),
            };
        } else if let Some(v) = a.strip_prefix("--t2b5-fail-create=") {
            o.fail_create = attempts(v)?;
        } else if let Some(v) = a.strip_prefix("--t2b5-fail-after-build=") {
            o.fail_after_build = attempts(v)?;
        } else if a.starts_with("--t2b") {
            return Err("Unknown experiment option".into());
        }
    }
    Ok(o)
}
fn attempts(v: &str) -> Result<Vec<u64>, String> {
    v.split(',')
        .map(|n| {
            n.parse::<u64>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| "Injection attempts must be positive integers".into())
        })
        .collect()
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Absent,
    Creating,
    Present,
    Hidden,
    Destroying,
    Failed,
}
pub struct Lifecycle {
    pub mode: Mode,
    pub phase: Phase,
    pub label: Option<String>,
    pub attempts: u64,
    generation: u64,
    reopen: bool,
    failed_cleanup: bool,
}
impl Lifecycle {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            phase: Phase::Absent,
            label: None,
            attempts: 0,
            generation: 1,
            reopen: false,
            failed_cleanup: false,
        }
    }
    pub fn begin_show(&mut self) -> Option<(String, bool)> {
        if self.phase == Phase::Destroying {
            self.reopen = true;
            return None;
        }
        if self.phase == Phase::Creating || (self.phase == Phase::Failed && self.label.is_some()) {
            return None;
        }
        let create = self.label.is_none();
        if create {
            self.attempts += 1;
            self.label = Some(format!("main-{}", self.generation));
        }
        self.phase = Phase::Creating;
        Some((self.label.clone().unwrap(), create))
    }
    pub fn shown(&mut self, ok: bool) {
        self.phase = if ok { Phase::Present } else { Phase::Failed };
    }
    pub fn create_failed(&mut self, partial: bool) {
        self.failed_cleanup = partial;
        self.phase = if partial {
            Phase::Destroying
        } else {
            self.label = None;
            Phase::Failed
        };
        self.reopen = false;
    }
    pub fn cleanup_failed(&mut self) {
        self.phase = Phase::Failed;
    }
    pub fn retry_cleanup(&mut self) {
        self.failed_cleanup = true;
        self.reopen = true;
        self.phase = Phase::Destroying;
    }
    pub fn stored(&mut self) {
        self.failed_cleanup = false;
        self.phase = match self.mode {
            Mode::Hide => Phase::Hidden,
            Mode::Destroy => Phase::Destroying,
        };
    }
    pub fn destroyed(&mut self, label: &str) -> bool {
        if self.label.as_deref() != Some(label) {
            return false;
        }
        self.label = None;
        self.generation += 1;
        self.phase = if self.failed_cleanup {
            Phase::Failed
        } else {
            Phase::Absent
        };
        self.failed_cleanup = false;
        std::mem::take(&mut self.reopen)
    }
}
pub fn keep_alive(code: Option<i32>, stopping: bool) -> bool {
    code.is_none() && !stopping
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_and_injection_validation() {
        let a = |s: &[&str]| s.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let o = options(&a(&["app"])).unwrap();
        assert_eq!(o.mode.unwrap_or_default(), Mode::Destroy);
        assert_eq!(
            options(&a(&["app", "--t2b2-window-mode=hide"]))
                .unwrap()
                .mode
                .unwrap_or_default(),
            Mode::Hide
        );
        assert!(o.fail_create.is_empty());
        assert!(options(&a(&["app", "--t2b5-fail-create=0"])).is_err());
        assert_eq!(
            options(&a(&["app", "--t2b5-fail-create=1,3"]))
                .unwrap()
                .fail_create,
            vec![1, 3]
        );
    }
    #[test]
    fn first_failure_and_retry() {
        let mut l = Lifecycle::new(Mode::Hide);
        assert_eq!(l.phase, Phase::Absent);
        let (a, c) = l.begin_show().unwrap();
        assert!(c);
        assert!(l.begin_show().is_none());
        l.create_failed(false);
        assert_eq!(l.phase, Phase::Failed);
        let (b, c) = l.begin_show().unwrap();
        assert!(c);
        assert_eq!(a, b);
        l.shown(true);
        assert_eq!(l.phase, Phase::Present);
    }
    #[test]
    fn partial_failure_blocks_until_destroyed() {
        let mut l = Lifecycle::new(Mode::Destroy);
        let (a, _) = l.begin_show().unwrap();
        l.create_failed(true);
        assert!(l.begin_show().is_none());
        assert!(l.destroyed(&a));
        assert!(l.begin_show().is_some());
    }
    #[test]
    fn stale_destroy_cannot_remove_new_window() {
        let mut l = Lifecycle::new(Mode::Destroy);
        let (a, _) = l.begin_show().unwrap();
        l.shown(true);
        l.stored();
        assert!(!l.destroyed(&a));
        let (b, _) = l.begin_show().unwrap();
        assert!(!l.destroyed(&a));
        assert_eq!(l.label.as_deref(), Some(b.as_str()));
        assert_eq!(l.phase, Phase::Creating);
    }
    #[test]
    fn cleanup_failure_retains_reference_without_duplicate() {
        let mut l = Lifecycle::new(Mode::Destroy);
        let (a, _) = l.begin_show().unwrap();
        l.create_failed(true);
        l.cleanup_failed();
        assert!(l.begin_show().is_none());
        l.retry_cleanup();
        assert!(l.destroyed(&a));
        assert!(l.begin_show().unwrap().1);
    }
    #[test]
    fn hide_reuses_generation() {
        let mut l = Lifecycle::new(Mode::Hide);
        let (a, _) = l.begin_show().unwrap();
        l.shown(true);
        l.stored();
        let (b, c) = l.begin_show().unwrap();
        assert!(!c);
        assert_eq!(a, b);
        assert_eq!(l.attempts, 1);
    }
    #[test]
    fn stalled_creation_is_not_unlocked_by_open() {
        let mut l = Lifecycle::new(Mode::Destroy);
        l.begin_show();
        for _ in 0..100 {
            assert!(l.begin_show().is_none());
        }
        assert_eq!(l.attempts, 1);
        assert_eq!(l.phase, Phase::Creating);
    }
    #[test]
    fn exit_policy() {
        assert!(keep_alive(None, false));
        assert!(!keep_alive(None, true));
        assert!(!keep_alive(Some(0), false));
    }
}
