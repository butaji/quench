//! Compile-time build contract for production runtime artifacts.
//!
//! Cargo owns optimization policy; this module exposes the selected profile so
//! executable hosts and focused smoke tests cannot silently run an unoptimized
//! artifact while claiming production behavior.

/// Profile selected by Cargo for this runtime artifact.
pub const PROFILE: &str = env!("QUENCH_BUILD_PROFILE");

/// Whether this artifact was built with a production Cargo profile.
#[cfg(quench_production)]
pub const IS_PRODUCTION: bool = true;
#[cfg(not(quench_production))]
pub const IS_PRODUCTION: bool = false;

/// Return the profile contract as one canonical value for host diagnostics.
pub const fn contract() -> (&'static str, bool) {
    (PROFILE, IS_PRODUCTION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_contract_is_nonempty_and_consistent() {
        assert!(!PROFILE.is_empty());
        let expected = PROFILE == "release" || PROFILE == "production" || PROFILE == "release-thin";
        assert_eq!(IS_PRODUCTION, expected);
    }
}
