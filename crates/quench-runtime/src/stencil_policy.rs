//! Host capabilities for the physical stencil views.
//!
//! Semantic instructions remain architecture-independent.  This module is the
//! single edge where compile-time ISA facts and the explicit ARM development
//! opt-in become an immutable execution policy.  Plan construction derives all
//! physical views from that policy instead of repeating target branches.

use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Architecture {
    X86_64,
    Aarch64,
    Other,
}

const fn architecture() -> Architecture {
    #[cfg(target_arch = "x86_64")]
    {
        Architecture::X86_64
    }
    #[cfg(target_arch = "aarch64")]
    {
        Architecture::Aarch64
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        Architecture::Other
    }
}

/// Physical views available to one host build.
///
/// These are capabilities, not alternate JavaScript semantics.  The policy is
/// computed once, then every baseline/optimizing plan derives its admission
/// map from the same fact.  Fused regions are deliberately separate from
/// scalar leaves because their current ARM implementation still crosses a
/// Rust handler bridge for each operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExecutionPolicy {
    pub(crate) native_leaves: bool,
    pub(crate) native_dispatch: bool,
    pub(crate) fused_regions: bool,
    /// Narrow set of regions with a physical composed executor. ARM exposes
    /// this only through the explicit development opt-in.
    pub(crate) composed_regions: bool,
    pub(crate) optimizing_view: bool,
}

impl ExecutionPolicy {
    pub(crate) const fn allows_admission(self) -> bool {
        self.native_leaves
            || self.native_dispatch
            || self.fused_regions
            || self.composed_regions
    }

    #[cfg(test)]
    pub(crate) fn arm_opt_in_for_test() -> Self {
        Self::from_architecture(Architecture::Aarch64, true)
    }

    /// Exercise a helper-capable region through the normal baseline driver
    /// without enabling it in the production AArch64 policy.
    #[cfg(test)]
    pub(crate) fn bridge_opt_in_for_test() -> Self {
        Self {
            native_leaves: false,
            native_dispatch: false,
            fused_regions: true,
            composed_regions: false,
            optimizing_view: false,
        }
    }

    fn from_architecture(arch: Architecture, arm_opt_in: bool) -> Self {
        match arch {
            Architecture::X86_64 => Self {
                native_leaves: true,
                native_dispatch: true,
                fused_regions: true,
                composed_regions: true,
                optimizing_view: true,
            },
            Architecture::Aarch64 => Self {
                native_leaves: arm_opt_in,
                native_dispatch: false,
                fused_regions: false,
                composed_regions: arm_opt_in,
                // The explicit opt-in now exposes only the bounded composed
                // region and scalar leaves whose generated entries are
                // available on this ISA; the default remains conservative.
                optimizing_view: arm_opt_in,
            },
            Architecture::Other => Self {
                native_leaves: false,
                native_dispatch: false,
                fused_regions: false,
                composed_regions: false,
                optimizing_view: false,
            },
        }
    }

    fn current_uncached() -> Self {
        let arm_opt_in = std::env::var_os("QUENCH_ENABLE_AARCH64_STENCILS").is_some();
        Self::from_architecture(architecture(), arm_opt_in)
    }
}

static CURRENT: OnceLock<ExecutionPolicy> = OnceLock::new();

pub(crate) fn current() -> ExecutionPolicy {
    *CURRENT.get_or_init(ExecutionPolicy::current_uncached)
}

#[cfg(test)]
mod tests {
    use super::{Architecture, ExecutionPolicy};

    #[test]
    fn policy_is_a_derived_capability_set() {
        assert_eq!(
            ExecutionPolicy::from_architecture(Architecture::X86_64, false),
            ExecutionPolicy {
                native_leaves: true,
                native_dispatch: true,
                fused_regions: true,
                composed_regions: true,
                optimizing_view: true,
            }
        );
        assert_eq!(
            ExecutionPolicy::from_architecture(Architecture::Aarch64, false),
            ExecutionPolicy {
                native_leaves: false,
                native_dispatch: false,
                fused_regions: false,
                composed_regions: false,
                optimizing_view: false,
            }
        );
        assert_eq!(
            ExecutionPolicy::from_architecture(Architecture::Aarch64, true),
            ExecutionPolicy {
                native_leaves: true,
                native_dispatch: false,
                fused_regions: false,
                composed_regions: true,
                optimizing_view: true,
            }
        );
        assert!(!ExecutionPolicy::from_architecture(Architecture::Aarch64, false)
            .allows_admission());
        assert!(ExecutionPolicy::from_architecture(Architecture::Aarch64, true)
            .allows_admission());
    }
}
