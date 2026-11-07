//! The VM's boolean environment switches: one definition each, read once per process.
//!
//! A switch is an A/B arm or a diagnostic, never semantics: every arm runs the same program to
//! the same result. `1`/`true` turns one on, `0`/`false` off, and anything else (or nothing)
//! leaves its default. A compiled call consults several, and reading the environment on each
//! call cost more than the calls (HashSet-wasm `--jit`: `getenv` was 68% of the run's
//! instructions), so each is read once.

pub(super) struct Switch {
    pub(super) name: &'static str,
    default: bool,
    value: std::sync::OnceLock<Option<bool>>,
}

impl Switch {
    const fn new(name: &'static str, default: bool) -> Self {
        Self { name, default, value: std::sync::OnceLock::new() }
    }

    /// The environment's setting, if it names one.
    fn set(&self) -> Option<bool> {
        #[cfg(test)]
        if let Some(enabled) = OVERRIDES.with(|overrides| {
            overrides.borrow().iter().rev().find(|(name, _)| *name == self.name).map(|(_, enabled)| *enabled)
        }) {
            return Some(enabled);
        }
        *self.value.get_or_init(|| match std::env::var(self.name).as_deref() {
            Ok("1" | "true") => Some(true),
            Ok("0" | "false") => Some(false),
            _ => None,
        })
    }

    pub(super) fn enabled(&self) -> bool {
        self.set().unwrap_or(self.default)
    }

}

#[cfg(test)]
thread_local! {
    static OVERRIDES: std::cell::RefCell<Vec<(&'static str, bool)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Runs `run` with `switch` set to `enabled` on this thread.
#[cfg(test)]
pub(super) fn with_switch<R>(switch: &Switch, enabled: bool, run: impl FnOnce() -> R) -> R {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            OVERRIDES.with(|overrides| overrides.borrow_mut().pop());
        }
    }
    OVERRIDES.with(|overrides| overrides.borrow_mut().push((switch.name, enabled)));
    let _restore = Restore;
    run()
}

/// Wasm modules stay selective for good, compiling only their hot roots
/// (`WasmModuleCompile::NEVER`): an A/B arm against the default, which compiles a long runner
/// whole.
pub(super) static WASM_JIT_SELECTIVE: Switch = Switch::new("QUENCH_WASM_JIT_SELECTIVE", false);
pub(super) static WASM_JIT_ROOT_ONLY: Switch = Switch::new("QUENCH_WASM_JIT_ROOT_ONLY", false);
pub(super) static WASM_JIT_ROOT_FIRST: Switch = Switch::new("QUENCH_WASM_JIT_ROOT_FIRST", false);
/// TOD-191's correctness oracle may force hot candidates past the payback model so guarded
/// exits can be tested independently of profitability. Diagnostic only; never set by score
/// harnesses.
pub(super) static JS_JIT_UNPRICED_GENERAL_DISPATCH: Switch = Switch::new("QUENCH_JS_JIT_UNPRICED_GENERAL_DISPATCH", false);
/// Per-function JIT diagnostics and the reports they print at exit.
pub(super) static JS_JIT_STATS: Switch = Switch::new("QUENCH_JS_JIT_STATS", false);
pub(super) static JS_JIT_GENERAL_DISPATCH: Switch = Switch::new("QUENCH_JS_JIT_GENERAL_DISPATCH", false);
pub(super) static JS_JIT_LANE_METHODS: Switch = Switch::new("QUENCH_JS_JIT_LANE_METHODS", false);

#[cfg(test)]
const ALL: &[&Switch] = &[
    &WASM_JIT_SELECTIVE, &WASM_JIT_ROOT_ONLY, &WASM_JIT_ROOT_FIRST,
    &JS_JIT_UNPRICED_GENERAL_DISPATCH, &JS_JIT_STATS, &JS_JIT_GENERAL_DISPATCH, &JS_JIT_LANE_METHODS,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switches_are_named_once_and_overrides_nest() {
        let mut names: Vec<_> = ALL.iter().map(|switch| switch.name).collect();
        assert!(names.iter().all(|name| name.starts_with("QUENCH_")));
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ALL.len(), "each switch has one definition");
        let switch = &JS_JIT_LANE_METHODS;
        with_switch(switch, true, || {
            assert!(switch.enabled());
            with_switch(switch, false, || assert!(!switch.enabled()));
            assert!(switch.enabled());
        });
    }

    /// No other file reads one of these switches' variables itself.
    #[test]
    fn switch_variables_are_read_only_here() {
        let sources = [
            include_str!("compiled.rs"),
            include_str!("program_store.rs"),
            include_str!("wasm_js.rs"),
        ];
        for switch in ALL {
            let quoted = format!("\"{}\"", switch.name);
            for source in sources {
                assert!(!source.contains(&quoted), "{} is read outside `switches`", switch.name);
            }
        }
    }
}
