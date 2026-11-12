//! The JIT modes a process can run in, and the tiers each one enables: the one place the flags
//! `--no-jit`, `--b-jit` and `--u-jit` are defined (`--q-jit` is reserved for the Cranelift quick mode and rejected until it lands).
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

    /// Record `flag` as the process's mode in `chosen`: an error message if a mode was already
    /// chosen or the flag is the removed `--jit`.
    pub fn select(chosen: &mut Option<Self>, flag: &str) -> Result<(), &'static str> {
        match (chosen.is_some(), Self::from_flag(flag)) {
            (true, _) => Err("only one of --no-jit, --b-jit, --q-jit and --u-jit can be used"),
            (false, Some(mode)) => Ok(*chosen = Some(mode)),
            (false, None) if flag == "--jit" => Err("--jit was removed: JIT is the default, and --no-jit selects the interpreter"),
            (false, None) => Err("unknown JIT flag: use --no-jit, --b-jit, --q-jit or --u-jit"),
        }
    }

    /// Whether the (BaseJIT, UltraJIT) tiers are enabled.
    pub fn tiers_enabled(self) -> (bool, bool) {
        match self {
            Self::Interp => (false, false),
            Self::BaseOnly => (true, false),
            Self::UltraOnly | Self::QuickOnly => (false, true),
            Self::Full => (true, true),
        }
    }

    /// The execution mode the runtime is set to: `--b-jit` runs the compiled lane with its loop
    /// regions built from BaseJIT stencils only (`base_only`).
    pub fn execution_mode(self) -> ExecutionMode {
        match self.tiers_enabled() {
            (false, false) => ExecutionMode::Interpreted,
            _ => ExecutionMode::Compiled,
        }
    }

    /// Makes this the process's mode, which `base_only` and `base_regions` report.
    pub fn set_process_mode(self) {
        PROCESS_MODE.store(self as u8, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Whether the Cranelift function tiers (lane bodies, their background compiles and entry-cell
/// publication, the JavaScript function tiers) run: the one `tiers_enabled` fact of the process's mode,
/// UltraJIT's half. `--b-jit` is the interpreter plus stencil regions.
pub fn function_tiers() -> bool {
    let mode = [JitMode::Interp, JitMode::BaseOnly, JitMode::UltraOnly, JitMode::QuickOnly, JitMode::Full]
        .into_iter()
        .find(|mode| *mode as u8 == PROCESS_MODE.load(std::sync::atomic::Ordering::Relaxed))
        .unwrap_or_default();
    mode.tiers_enabled().1
}

static PROCESS_MODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(JitMode::Full as u8);

/// Whether the process runs `--b-jit`: loop regions that BaseJIT declines are not compiled by
/// UltraJIT either.
pub fn base_only() -> bool {
    PROCESS_MODE.load(std::sync::atomic::Ordering::Relaxed) == JitMode::BaseOnly as u8
}

/// Whether the process runs `--q-jit`: every Cranelift compile is `CompileLevel::Quick`.
pub fn quick_only() -> bool {
    PROCESS_MODE.load(std::sync::atomic::Ordering::Relaxed) == JitMode::QuickOnly as u8
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
        let tiers = |flag| JitMode::from_flag(flag).map(JitMode::tiers_enabled);
        assert_eq!(tiers("--no-jit"), Some((false, false)));
        assert_eq!(tiers("--b-jit"), Some((true, false)));
        assert_eq!(tiers("--q-jit"), None);
        assert_eq!(tiers("--f-jit"), None);
        assert_eq!(tiers("--u-jit"), Some((false, true)));
        assert_eq!(tiers("--jit"), None);
        assert_eq!(JitMode::default().tiers_enabled(), (true, true));
    }
}
