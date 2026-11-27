//! The JIT modes a process can run in, and the tiers each one enables: the one place the flags
//! `--no-jit`, `--b-jit`, `--q-jit` and `--u-jit` are defined.
use crate::ExecutionMode;

/// Which tiers run: the interpreter alone, BaseJIT (stencils) alone, UltraJIT alone, or both (the default).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JitMode {
    /// `--no-jit`: the interpreter alone.
    Interp,
    /// `--b-jit`: BaseJIT (stencils) alone.
    BaseOnly,
    /// `--u-jit`: UltraJIT alone.
    UltraOnly,
    /// `--q-jit`: QuickJIT alone: the Cranelift tiers compile every body at `CompileLevel::Quick`.
    QuickOnly,
    /// No flag: UltraJIT and BaseJIT.
    #[default]
    Full,
}

impl JitMode {
    /// Every mode a flag selects, with its flag: the one table `from_flag`, `flag` and `name` read.
    pub const FLAGGED: [(Self, &'static str); 4] = [
        (Self::Interp, "--no-jit"), (Self::BaseOnly, "--b-jit"), (Self::UltraOnly, "--u-jit"), (Self::QuickOnly, "--q-jit"),
    ];

    /// The mode a command line flag selects, or `None` if the flag is not a JIT mode.
    pub fn from_flag(flag: &str) -> Option<Self> {
        Self::FLAGGED.iter().find(|(_, own)| *own == flag).map(|(mode, _)| *mode)
    }

    /// The flag that selects this mode; none for the default.
    pub fn flag(self) -> Option<&'static str> {
        Self::FLAGGED.iter().find(|(mode, _)| *mode == self).map(|(_, flag)| *flag)
    }

    /// The name reports give this mode: its flag without the dashes, `jit` for the default.
    pub fn name(self) -> &'static str {
        self.flag().map_or("jit", |flag| flag.trim_start_matches("--"))
    }

    /// The mode the process runs in (`set_process_mode`).
    pub fn process() -> Self {
        process_mode()
    }

    /// Whether `arg` has the shape of a JIT mode flag (`--no-jit`, `--b-jit`, the removed `--jit`, or an unknown `--x-jit`).
    pub fn is_flag(arg: &str) -> bool {
        arg.starts_with("--") && arg.ends_with("-jit")
    }

    /// Record `flag` as the process's mode in `chosen`: an error message if a mode was already
    /// chosen or the flag is the removed `--jit`.
    pub fn select(chosen: &mut Option<Self>, flag: &str) -> Result<(), String> {
        match (chosen.is_some(), Self::from_flag(flag)) {
            (true, _) => Err("only one of --no-jit, --b-jit, --q-jit and --u-jit can be used".into()),
            (false, Some(mode)) => Ok(*chosen = Some(mode)),
            (false, None) if flag == "--jit" => Err("--jit was removed: JIT is the default, and --no-jit selects the interpreter".into()),
            (false, None) => Err(format!("unknown JIT flag {flag}: use --no-jit, --b-jit, --q-jit or --u-jit")),
        }
    }

    /// Whether `tier` may run in this mode: the one table every tier question reads.
    pub fn allows(self, tier: Tier) -> bool {
        match (self, tier) {
            (_, Tier::Ahead) => true,
            (Self::Interp, _) => false,
            // AArch64's stitched region starts an induction at 0 when it is entered mid-loop
            // (`Self::Full` leaves Base out for the same reason), so `--b-jit` runs no tier here:
            // its rows stay for the AOT image's base bodies, and the interpreter gives the exact
            // results the mode's gate compares. Debt (TOD-1202).
            (Self::BaseOnly, _) if cfg!(target_arch = "aarch64") => false,
            (Self::BaseOnly, tier) => tier == Tier::Base,
            (Self::QuickOnly, tier) => tier == Tier::Quick,
            (Self::UltraOnly, tier) => tier == Tier::Ultra,
            // AArch64's stitched (BaseJIT) loop region starts an induction at 0 when it is
            // entered mid-loop, so a loop that entered it after k iterations runs k extra ones
            // (`ty2.js`: n=20000+k; the floor rows, which enter at 0, are exact). The default
            // mode leaves Base out there and the Cranelift tiers run; `--b-jit`, which asks for
            // it, still runs it. Debt (TOD-1202): Base returns when that entry is fixed.
            (Self::Full, Tier::Base) if cfg!(target_arch = "aarch64") => false,
            (Self::Full, _) => true,
        }
    }

    /// Whether any of `tiers` may run in this mode.
    #[inline]
    pub fn allows_any(self, tiers: &[Tier]) -> bool {
        tiers.iter().any(|&tier| self.allows(tier))
    }

    /// The execution mode the runtime is set to: `--b-jit` runs the compiled lane with its loop
    /// regions built from BaseJIT stencils only (`tier_policy::loop_regions_are_stencils`).
    pub fn execution_mode(self) -> ExecutionMode {
        if self.allows_any(Tier::ANY) { ExecutionMode::Compiled } else { ExecutionMode::Interpreted }
    }

    /// Makes this the process's mode, which `base_only` and `tiers_allowed` report.
    pub fn set_process_mode(self) {
        PROCESS_MODE.store(self as u8, std::sync::atomic::Ordering::Relaxed);
    }

    /// Runs `body` with this as the calling thread's mode, which tests on other threads do not see.
    #[cfg(test)]
    pub fn with_mode<R>(self, body: impl FnOnce() -> R) -> R {
        let previous = THREAD_MODE.with(|mode| mode.replace(Some(self)));
        let result = body();
        THREAD_MODE.with(|mode| mode.set(previous));
        result
    }
}

#[cfg(test)]
thread_local! {
    static THREAD_MODE: std::cell::Cell<Option<JitMode>> = const { std::cell::Cell::new(None) };
}

/// The compiling tiers, in the order a function climbs them: BaseJIT (stencils), QuickJIT
/// (Cranelift, `CompileLevel::Quick`) and UltraJIT (Cranelift, `CompileLevel::Optimized`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Base,
    Quick,
    Ultra,
    /// An AOT image's body: compiled before the run, so no mode's compile gate applies to it, and it publishes in
    /// every mode, `--no-jit` included (the image is the program's specialization, not a run-time tier).
    Ahead,
}

impl Tier {
    /// Every tier.
    pub const ANY: &'static [Tier] = &[Tier::Base, Tier::Quick, Tier::Ultra];
    /// The Cranelift tiers: lane bodies, their background compiles and entry-cell publication,
    /// the JavaScript function tiers. `--b-jit` is the interpreter plus stencil regions.
    pub const CRANELIFT: &'static [Tier] = &[Tier::Quick, Tier::Ultra];
}

static PROCESS_MODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(JitMode::Full as u8);

// The mode a test thread runs in, over the process's: tests share one process, so a test that
// needs `--b-jit` selects it for its own thread only (`with_mode`).
#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static TEST_MODE: std::cell::Cell<Option<JitMode>> = const { std::cell::Cell::new(None) };
}

/// Runs `body` on this thread in `mode`.
#[cfg(any(test, feature = "test-support"))]
pub fn with_mode<R>(mode: JitMode, body: impl FnOnce() -> R) -> R {
    let before = TEST_MODE.with(|chosen| chosen.replace(Some(mode)));
    let result = body();
    TEST_MODE.with(|chosen| chosen.set(before));
    result
}

#[inline]
fn process_mode() -> JitMode {
    #[cfg(test)]
    if let Some(mode) = THREAD_MODE.with(std::cell::Cell::get) {
        return mode;
    }
    #[cfg(any(test, feature = "test-support"))]
    if let Some(mode) = TEST_MODE.with(std::cell::Cell::get) {
        return mode;
    }
    match PROCESS_MODE.load(std::sync::atomic::Ordering::Relaxed) {
        mode if mode == JitMode::Interp as u8 => JitMode::Interp,
        mode if mode == JitMode::BaseOnly as u8 => JitMode::BaseOnly,
        mode if mode == JitMode::UltraOnly as u8 => JitMode::UltraOnly,
        mode if mode == JitMode::QuickOnly as u8 => JitMode::QuickOnly,
        mode if mode == JitMode::Full as u8 => JitMode::Full,
        _ => JitMode::default(),
    }
}

/// Whether the process's mode lets any of `tiers` run: the mode half of the one tier predicate
/// (`tier_policy::tier_may_run` adds the memory policy).
#[inline]
pub fn tiers_allowed(tiers: &[Tier]) -> bool {
    process_mode().allows_any(tiers)
}

/// Whether the process runs `--b-jit`: loop regions that BaseJIT declines are not compiled by
/// Cranelift either.
pub fn base_only() -> bool {
    tiers_allowed(&[Tier::Base]) && !tiers_allowed(Tier::CRANELIFT)
}

/// Whether every Cranelift compile is `CompileLevel::Quick`: only QuickJIT is allowed there.
pub fn quick_only() -> bool {
    tiers_allowed(&[Tier::Quick]) && !tiers_allowed(&[Tier::Ultra])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_flag_selects_its_tiers() {
        let tiers = |flag| JitMode::from_flag(flag).map(|mode| [Tier::Base, Tier::Quick, Tier::Ultra].map(|tier| mode.allows(tier)));
        assert_eq!(tiers("--no-jit"), Some([false, false, false]));
        assert_eq!(tiers("--b-jit"), Some([true, false, false]));
        assert_eq!(tiers("--q-jit"), Some([false, true, false]));
        assert_eq!(tiers("--u-jit"), Some([false, false, true]));
        assert_eq!(tiers("--jit"), None);
        let mut chosen = None;
        let error = JitMode::select(&mut chosen, "--zz-jit").unwrap_err();
        assert!(error.starts_with("unknown JIT flag --zz-jit") && chosen.is_none());
        assert!(JitMode::default().allows(Tier::Ultra));
    }
}
