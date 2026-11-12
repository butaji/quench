//! The JIT modes a process can run in, and the tiers each one enables: the one place the flags
//! `--no-jit`, `--b-jit` and `--u-jit` are defined (`--q-jit` is reserved for the Cranelift
//! quick mode and rejected until it lands).
use crate::ExecutionMode;

/// Which tiers run: the interpreter alone, BaseJIT (stencils) alone, UltraJIT alone, or both
/// (the default).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JitMode {
    /// `--no-jit`: the interpreter alone.
    Interp,
    /// `--b-jit`: BaseJIT (stencils) alone.
    BaseOnly,
    /// `--u-jit`: UltraJIT alone.
    UltraOnly,
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
            _ => None,
        }
    }

    /// Record `flag` as the process's mode in `chosen`: an error message if a mode was already
    /// chosen, or the flag names a removed or not-yet-built mode.
    pub fn select(chosen: &mut Option<Self>, flag: &str) -> Result<(), &'static str> {
        match (chosen.is_some(), Self::from_flag(flag)) {
            (_, None) if flag == "--q-jit" => Err("--q-jit (Cranelift quick mode) is not available yet; --b-jit selects the stencil tier"),
            (true, _) => Err("only one of --no-jit, --b-jit and --u-jit can be used"),
            (false, Some(mode)) => Ok(*chosen = Some(mode)),
            (false, None) if flag == "--jit" => Err("--jit was removed: JIT is the default, and --no-jit selects the interpreter"),
            (false, None) => Err("unknown JIT flag: use --no-jit, --b-jit or --u-jit"),
        }
    }

    /// Whether the (BaseJIT, UltraJIT) tiers are enabled.
    pub fn tiers_enabled(self) -> (bool, bool) {
        match self {
            Self::Interp => (false, false),
            Self::BaseOnly => (true, false),
            Self::UltraOnly => (false, true),
            Self::Full => (true, true),
        }
    }

    /// The execution mode the runtime is set to. The stencil tier is not built in this tree yet,
    /// so `--b-jit` runs the interpreter until it lands.
    pub fn execution_mode(self) -> ExecutionMode {
        match self.tiers_enabled() {
            (_, true) => ExecutionMode::Compiled,
            (_, false) => ExecutionMode::Interpreted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_flag_selects_its_tiers() {
        let tiers = |flag| JitMode::from_flag(flag).map(JitMode::tiers_enabled);
        assert_eq!(tiers("--no-jit"), Some((false, false)));
        assert_eq!(tiers("--b-jit"), Some((true, false)));
        assert_eq!(tiers("--u-jit"), Some((false, true)));
        assert_eq!(tiers("--jit"), None);
        assert_eq!(tiers("--f-jit"), None);
        assert_eq!(tiers("--q-jit"), None);
        assert_eq!(JitMode::default().tiers_enabled(), (true, true));
        let mut chosen = None;
        assert!(JitMode::select(&mut chosen, "--q-jit").is_err(), "the quick mode is reserved");
        assert!(JitMode::select(&mut chosen, "--f-jit").is_err(), "the old stencil flag is gone");
    }
}
