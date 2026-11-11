//! The JIT modes a process can run in, and the tiers each one enables: the one place the flags
//! `--no-jit`, `--f-jit` and `--u-jit` are defined.
use crate::ExecutionMode;

/// Which tiers run: the interpreter alone, FastJIT alone, UltraJIT alone, or both (the default).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JitMode {
    /// `--no-jit`: the interpreter alone.
    Interp,
    /// `--f-jit`: FastJIT alone.
    FastOnly,
    /// `--u-jit`: UltraJIT alone.
    UltraOnly,
    /// No flag: UltraJIT and FastJIT.
    #[default]
    Full,
}

impl JitMode {
    /// The mode a command line flag selects, or `None` if the flag is not a JIT mode.
    pub fn from_flag(flag: &str) -> Option<Self> {
        match flag {
            "--no-jit" => Some(Self::Interp),
            "--f-jit" => Some(Self::FastOnly),
            "--u-jit" => Some(Self::UltraOnly),
            _ => None,
        }
    }

    /// Record `flag` as the process's mode in `chosen`: an error message if a mode was already
    /// chosen or the flag is the removed `--jit`.
    pub fn select(chosen: &mut Option<Self>, flag: &str) -> Result<(), &'static str> {
        match (chosen.is_some(), Self::from_flag(flag)) {
            (true, _) => Err("only one of --no-jit, --f-jit and --u-jit can be used"),
            (false, Some(mode)) => Ok(*chosen = Some(mode)),
            (false, None) => Err("--jit was removed: JIT is the default, and --no-jit selects the interpreter"),
        }
    }

    /// Whether the (FastJIT, UltraJIT) tiers are enabled.
    pub fn tiers_enabled(self) -> (bool, bool) {
        match self {
            Self::Interp => (false, false),
            Self::FastOnly => (true, false),
            Self::UltraOnly => (false, true),
            Self::Full => (true, true),
        }
    }

    /// The execution mode the runtime is set to. FastJIT has no tier of its own yet, so
    /// `--f-jit` runs the interpreter until it lands.
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
        assert_eq!(tiers("--f-jit"), Some((true, false)));
        assert_eq!(tiers("--u-jit"), Some((false, true)));
        assert_eq!(tiers("--jit"), None);
        assert_eq!(JitMode::default().tiers_enabled(), (true, true));
    }
}
