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
    /// The mode a command line flag selects, or `None` if the flag is not a JIT mode.
    pub fn from_flag(flag: &str) -> Option<Self> {
        match flag {
            "--no-jit" => Some(Self::Interp),
            "--b-jit" => Some(Self::BaseOnly),
            "--u-jit" => Some(Self::UltraOnly),
            "--q-jit" => Some(Self::QuickOnly),
            _ => None,
        }
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
            (Self::Interp, _) => false,
            (Self::BaseOnly, tier) => tier == Tier::Base,
            (Self::QuickOnly, tier) => tier == Tier::Quick,
            (Self::UltraOnly, tier) => tier == Tier::Ultra,
            (Self::Full, _) => true,
        }
    }

    /// Whether any of `tiers` may run in this mode.
    pub fn allows_any(self, tiers: &[Tier]) -> bool {
        tiers.iter().any(|&tier| self.allows(tier))
    }

    /// The execution mode the runtime is set to: `--b-jit` runs the compiled lane with its loop
    /// regions built from BaseJIT stencils only (`base_only`).
    pub fn execution_mode(self) -> ExecutionMode {
        if self.allows_any(Tier::ANY) { ExecutionMode::Compiled } else { ExecutionMode::Interpreted }
    }

    /// Makes this the process's mode, which `base_only` and `base_regions` report.
    pub fn set_process_mode(self) {
        PROCESS_MODE.store(self as u8, std::sync::atomic::Ordering::Relaxed);
    }
}

/// The compiling tiers, in the order a function climbs them: BaseJIT (stencils), QuickJIT
/// (Cranelift, `CompileLevel::Quick`) and UltraJIT (Cranelift, `CompileLevel::Optimized`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Base,
    Quick,
    Ultra,
}

impl Tier {
    /// Every tier.
    pub const ANY: &'static [Tier] = &[Tier::Base, Tier::Quick, Tier::Ultra];
    /// The Cranelift tiers: lane bodies, their background compiles and entry-cell publication,
    /// the JavaScript function tiers. `--b-jit` is the interpreter plus stencil regions.
    pub const CRANELIFT: &'static [Tier] = &[Tier::Quick, Tier::Ultra];
}

static PROCESS_MODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(JitMode::Full as u8);

fn process_mode() -> JitMode {
    [JitMode::Interp, JitMode::BaseOnly, JitMode::UltraOnly, JitMode::QuickOnly, JitMode::Full]
        .into_iter()
        .find(|mode| *mode as u8 == PROCESS_MODE.load(std::sync::atomic::Ordering::Relaxed))
        .unwrap_or_default()
}

/// Whether the process's mode lets any of `tiers` run: the mode half of the one tier predicate
/// (`tier_policy::tier_may_run` adds the memory policy).
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

/// Whether loop regions are built from BaseJIT stencils: `--b-jit`, or `QUENCH_DEV=bjit`
/// while the tier is an experiment in the default mode.
pub fn base_regions() -> bool {
    base_only() || crate::trace::Dev::Bjit.on()
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
