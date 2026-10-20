//! Shared Cranelift target and executable-code backend for JavaScript and Wasm.
//!
//! Frontends retain their own bytecode semantics, but all tiers request a
//! target ISA from this cache and publish code through the same W^X memory
//! implementation in `compiled.rs`.

use cranelift_codegen::settings::{self, Configurable};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

type TargetIsa = dyn cranelift_codegen::isa::TargetIsa;

/// Reuse immutable host target configuration across JS function tiers and
/// Wasm module compilation. The verifier choice remains part of the key:
/// JavaScript currently verifies generated functions while the mature Wasm
/// compiler opts out after its own structural checks.
pub(super) fn target_isa(opt_level: &str, enable_verifier: bool) -> Option<Arc<TargetIsa>> {
    type IsaCache = HashMap<(String, bool), Arc<TargetIsa>>;
    static CACHE: OnceLock<Mutex<IsaCache>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = (opt_level.to_owned(), enable_verifier);
    let mut entries = cache.lock().ok()?;
    if let Some(isa) = entries.get(&key) {
        return Some(Arc::clone(isa));
    }

    let mut flags = settings::builder();
    flags.set("opt_level", opt_level).ok()?;
    flags
        .set(
            "enable_verifier",
            if enable_verifier { "true" } else { "false" },
        )
        .ok()?;
    let isa = cranelift_native::builder()
        .ok()?
        .finish(settings::Flags::new(flags))
        .ok()?;
    entries.insert(key, Arc::clone(&isa));
    Some(isa)
}
