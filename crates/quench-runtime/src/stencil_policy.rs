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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArmMode {
    Disabled,
    Leaves,
    Fusion,
    FusionNumeric,
    FusionProperty,
    FusionPredicate,
    Kernels,
    ArrayLoop,
    AffineLoop,
    Composed,
    All,
}

impl ArmMode {
    fn from_environment() -> Self {
        match std::env::var("QUENCH_AARCH64_STENCIL_MODE").as_deref() {
            Ok("leaves") => Self::Leaves,
            Ok("fusion") => Self::Fusion,
            Ok("fusion-numeric") => Self::FusionNumeric,
            Ok("fusion-property") => Self::FusionProperty,
            Ok("fusion-predicate") => Self::FusionPredicate,
            Ok("kernels") => Self::Kernels,
            Ok("array-loop") => Self::ArrayLoop,
            Ok("affine-loop") => Self::AffineLoop,
            Ok("composed") => Self::Composed,
            Ok("all") => Self::All,
            Ok(_) => Self::Disabled,
            Err(_) if std::env::var_os("QUENCH_ENABLE_AARCH64_STENCILS").is_some() => Self::All,
            Err(_) => Self::Disabled,
        }
    }
}

const fn local_fusion_policy(mode: ArmMode) -> LocalFusionPolicy {
    match mode {
        ArmMode::Fusion | ArmMode::All => LocalFusionPolicy::ALL,
        ArmMode::FusionNumeric => LocalFusionPolicy::NUMERIC_ONLY,
        ArmMode::FusionProperty => LocalFusionPolicy::PROPERTY_ONLY,
        ArmMode::FusionPredicate => LocalFusionPolicy::PREDICATE_ONLY,
        _ => LocalFusionPolicy::NONE,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LocalFusionPolicy(u8);

impl LocalFusionPolicy {
    const NUMERIC: u8 = 1 << 0;
    const PROPERTY: u8 = 1 << 1;
    const PREDICATE: u8 = 1 << 2;

    pub(crate) const NONE: Self = Self(0);
    pub(crate) const ALL: Self = Self(Self::NUMERIC | Self::PROPERTY | Self::PREDICATE);
    const NUMERIC_ONLY: Self = Self(Self::NUMERIC);
    const PROPERTY_ONLY: Self = Self(Self::PROPERTY);
    const PREDICATE_ONLY: Self = Self(Self::PREDICATE);

    pub(crate) const fn any(self) -> bool {
        self.0 != 0
    }

    pub(crate) const fn numeric(self) -> bool {
        self.0 & Self::NUMERIC != 0
    }

    pub(crate) const fn property(self) -> bool {
        self.0 & Self::PROPERTY != 0
    }

    pub(crate) const fn predicate(self) -> bool {
        self.0 & Self::PREDICATE != 0
    }
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
    pub(crate) local_fusions: LocalFusionPolicy,
    pub(crate) native_dispatch: bool,
    pub(crate) fused_regions: bool,
    pub(crate) array_kernels: bool,
    pub(crate) array_numeric_loops: bool,
    pub(crate) affine_i32_loops: bool,
    pub(crate) optimizing_view: bool,
}

impl ExecutionPolicy {
    pub(crate) const fn allows_admission(self) -> bool {
        self.native_leaves
            || self.local_fusions.any()
            || self.native_dispatch
            || self.fused_regions
            || self.array_kernels
            || self.array_numeric_loops
            || self.affine_i32_loops
    }

    /// Local fusions use leaf templates as implementation components without
    /// admitting the same templates as one-operation entries.
    pub(crate) const fn with_leaf_dependencies(mut self) -> Self {
        self.native_leaves = true;
        self
    }

    pub(crate) const fn allows_region_abi(self, abi: crate::stencil_select::RegionAbi) -> bool {
        use crate::stencil_select::RegionAbi;
        match abi {
            RegionAbi::Bridge => self.fused_regions,
            RegionAbi::ArrayKernel => self.array_kernels,
            RegionAbi::ArrayNumericLoop => self.array_numeric_loops,
            RegionAbi::AffineI32Loop => self.affine_i32_loops,
            _ => false,
        }
    }

    #[cfg(test)]
    pub(crate) fn arm_opt_in_for_test() -> Self {
        let mut policy = Self::from_architecture(Architecture::Aarch64, true);
        policy.local_fusions = LocalFusionPolicy::ALL;
        policy
    }

    #[cfg(test)]
    pub(crate) fn arm_composed_opt_in_for_test() -> Self {
        Self::from_architecture_and_mode(Architecture::Aarch64, ArmMode::Composed)
    }

    /// Exercise a helper-capable region through the normal baseline driver
    /// without enabling it in the production AArch64 policy.
    #[cfg(test)]
    pub(crate) fn bridge_opt_in_for_test() -> Self {
        Self {
            native_leaves: false,
            local_fusions: LocalFusionPolicy::NONE,
            native_dispatch: false,
            fused_regions: true,
            array_kernels: false,
            array_numeric_loops: false,
            affine_i32_loops: false,
            optimizing_view: false,
        }
    }

    fn from_architecture(arch: Architecture, arm_opt_in: bool) -> Self {
        let arm_mode = if arm_opt_in {
            ArmMode::All
        } else {
            ArmMode::Disabled
        };
        Self::from_architecture_and_mode(arch, arm_mode)
    }

    fn from_architecture_and_mode(arch: Architecture, arm_mode: ArmMode) -> Self {
        match arch {
            Architecture::X86_64 => Self {
                native_leaves: true,
                local_fusions: LocalFusionPolicy::ALL,
                native_dispatch: true,
                fused_regions: true,
                array_kernels: true,
                array_numeric_loops: true,
                affine_i32_loops: true,
                optimizing_view: true,
            },
            Architecture::Aarch64 => Self {
                native_leaves: matches!(arm_mode, ArmMode::Leaves | ArmMode::All),
                local_fusions: local_fusion_policy(arm_mode),
                native_dispatch: false,
                fused_regions: false,
                array_kernels: matches!(
                    arm_mode,
                    ArmMode::Kernels | ArmMode::Composed | ArmMode::All
                ),
                array_numeric_loops: matches!(
                    arm_mode,
                    ArmMode::ArrayLoop | ArmMode::Composed | ArmMode::All
                ),
                affine_i32_loops: matches!(
                    arm_mode,
                    ArmMode::AffineLoop | ArmMode::Composed | ArmMode::All
                ),
                // The AArch64 optimizing driver is not a distinct physical
                // contract: enabling it can re-enter structured fragments at
                // the wrong semantic boundary. Keep the verified leaves and
                // composed regions independently exercisable, but reject this
                // unsupported combination until its continuation contract is
                // proven end to end.
                optimizing_view: false,
            },
            Architecture::Other => Self {
                native_leaves: false,
                local_fusions: LocalFusionPolicy::NONE,
                native_dispatch: false,
                fused_regions: false,
                array_kernels: false,
                array_numeric_loops: false,
                affine_i32_loops: false,
                optimizing_view: false,
            },
        }
    }

    fn current_uncached() -> Self {
        Self::from_architecture_and_mode(architecture(), ArmMode::from_environment())
    }
}

static CURRENT: OnceLock<ExecutionPolicy> = OnceLock::new();

pub(crate) fn current() -> ExecutionPolicy {
    *CURRENT.get_or_init(ExecutionPolicy::current_uncached)
}

#[cfg(test)]
mod tests {
    use super::{Architecture, ArmMode, ExecutionPolicy};

    #[test]
    fn policy_is_a_derived_capability_set() {
        assert_eq!(
            ExecutionPolicy::from_architecture(Architecture::X86_64, false),
            ExecutionPolicy {
                native_leaves: true,
                local_fusions: super::LocalFusionPolicy::ALL,
                native_dispatch: true,
                fused_regions: true,
                array_kernels: true,
                array_numeric_loops: true,
                affine_i32_loops: true,
                optimizing_view: true,
            }
        );
        assert_eq!(
            ExecutionPolicy::from_architecture(Architecture::Aarch64, false),
            ExecutionPolicy {
                native_leaves: false,
                local_fusions: super::LocalFusionPolicy::NONE,
                native_dispatch: false,
                fused_regions: false,
                array_kernels: false,
                array_numeric_loops: false,
                affine_i32_loops: false,
                optimizing_view: false,
            }
        );
        assert_eq!(
            ExecutionPolicy::from_architecture(Architecture::Aarch64, true),
            ExecutionPolicy {
                native_leaves: true,
                local_fusions: super::LocalFusionPolicy::ALL,
                native_dispatch: false,
                fused_regions: false,
                array_kernels: true,
                array_numeric_loops: true,
                affine_i32_loops: true,
                optimizing_view: false,
            }
        );
        assert!(
            !ExecutionPolicy::from_architecture(Architecture::Aarch64, false).allows_admission()
        );
        assert!(ExecutionPolicy::from_architecture(Architecture::Aarch64, true).allows_admission());
    }

    #[test]
    fn arm_diagnostic_modes_isolate_physical_families() {
        let leaves =
            ExecutionPolicy::from_architecture_and_mode(Architecture::Aarch64, ArmMode::Leaves);
        let composed =
            ExecutionPolicy::from_architecture_and_mode(Architecture::Aarch64, ArmMode::Composed);
        assert!(leaves.native_leaves && !leaves.array_kernels);
        assert!(!leaves.local_fusions.any());
        let fusion =
            ExecutionPolicy::from_architecture_and_mode(Architecture::Aarch64, ArmMode::Fusion);
        assert!(fusion.local_fusions.any() && !fusion.native_leaves);
        assert_isolated_fusion_modes();
        assert!(!composed.native_leaves && composed.array_kernels);
        assert!(composed.array_numeric_loops && composed.affine_i32_loops);
        assert_isolated_region_modes();
        let all = ExecutionPolicy::from_architecture_and_mode(Architecture::Aarch64, ArmMode::All);
        assert!(!leaves.optimizing_view && !composed.optimizing_view);
        assert!(all.native_leaves && all.local_fusions.any() && all.array_kernels);
        assert!(all.array_numeric_loops && all.affine_i32_loops);
        assert!(!all.optimizing_view);
    }

    fn assert_isolated_region_modes() {
        use crate::stencil_select::RegionAbi;
        let policy =
            |mode| ExecutionPolicy::from_architecture_and_mode(Architecture::Aarch64, mode);
        let kernels = policy(ArmMode::Kernels);
        assert!(kernels.array_kernels && !kernels.array_numeric_loops);
        assert!(kernels.allows_region_abi(RegionAbi::ArrayKernel));
        assert!(!kernels.allows_region_abi(RegionAbi::ArrayNumericLoop));
        let array_loop = policy(ArmMode::ArrayLoop);
        assert!(array_loop.array_numeric_loops && !array_loop.affine_i32_loops);
        assert!(array_loop.allows_region_abi(RegionAbi::ArrayNumericLoop));
        assert!(!array_loop.allows_region_abi(RegionAbi::AffineI32Loop));
        let affine_loop = policy(ArmMode::AffineLoop);
        assert!(affine_loop.affine_i32_loops && !affine_loop.array_kernels);
        assert!(affine_loop.allows_region_abi(RegionAbi::AffineI32Loop));
        assert!(!affine_loop.allows_region_abi(RegionAbi::Bridge));
    }

    fn assert_isolated_fusion_modes() {
        let policy =
            |mode| ExecutionPolicy::from_architecture_and_mode(Architecture::Aarch64, mode);
        let numeric = policy(ArmMode::FusionNumeric).local_fusions;
        assert!(numeric.numeric() && !numeric.property() && !numeric.predicate());
        let property = policy(ArmMode::FusionProperty).local_fusions;
        assert!(!property.numeric() && property.property() && !property.predicate());
        let predicate = policy(ArmMode::FusionPredicate).local_fusions;
        assert!(!predicate.numeric() && !predicate.property() && predicate.predicate());
    }
}
