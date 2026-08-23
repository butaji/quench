//! Compile-time build contract for production runtime artifacts.
//!
//! Cargo owns optimization policy; this module exposes the selected profile
//! and LTO mode so executable hosts and focused smoke tests cannot silently
//! run an unoptimized artifact while claiming production behavior.

/// Profile selected by Cargo for this runtime artifact.
pub const PROFILE: &str = env!("QUENCH_BUILD_PROFILE");

/// LTO mode selected by the Cargo profile.
pub const LTO: &str = env!("QUENCH_BUILD_LTO");

/// Whether this artifact was built with a production Cargo profile.
#[cfg(quench_production)]
pub const IS_PRODUCTION: bool = true;
#[cfg(not(quench_production))]
pub const IS_PRODUCTION: bool = false;

/// The immutable build facts owned by this artifact.
///
/// The only invalid state is an unknown profile or LTO mode; `build.rs`
/// rejects those before compilation, so consumers never observe one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Contract {
    pub profile: &'static str,
    pub lto: &'static str,
    pub production: bool,
}

/// Return the canonical build contract for host diagnostics.
pub const fn contract() -> Contract {
    Contract {
        profile: PROFILE,
        lto: LTO,
        production: IS_PRODUCTION,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_contract_is_complete_and_consistent() {
        assert!(!PROFILE.is_empty());
        assert!(matches!(LTO, "off" | "thin" | "fat"));
        assert_eq!(
            IS_PRODUCTION,
            matches!(PROFILE, "release" | "production" | "release-thin")
        );
        if IS_PRODUCTION {
            assert!(matches!(LTO, "thin" | "fat"));
        }
        assert_eq!(
            contract(),
            Contract {
                profile: PROFILE,
                lto: LTO,
                production: IS_PRODUCTION
            }
        );
    }

    #[test]
    fn production_profile_is_fat_lto() {
        if PROFILE == "production" {
            assert_eq!(LTO, "fat");
            assert!(IS_PRODUCTION);
        }
    }
}
