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
/// Wasm module compilation, one per optimization level.
/// Whether Cranelift verifies the IR of every compile: in debug builds, where tests check
/// each lowering, and not in release, where the verifier cost about a quarter of every JIT
/// compile's instructions (npm semver/browserslist `--jit`, 2026-10-24).
const VERIFY_IR: bool = cfg!(debug_assertions);

pub(super) fn target_isa(opt_level: &str) -> Option<Arc<TargetIsa>> {
    type IsaCache = HashMap<String, Arc<TargetIsa>>;
    static CACHE: OnceLock<Mutex<IsaCache>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = opt_level.to_owned();
    let mut entries = cache.lock().ok()?;
    if let Some(isa) = entries.get(&key) {
        return Some(Arc::clone(isa));
    }

    let mut flags = settings::builder();
    flags.set("opt_level", opt_level).ok()?;
    flags
        .set("enable_verifier", if VERIFY_IR { "true" } else { "false" })
        .ok()?;
    let isa = cranelift_native::builder()
        .ok()?
        .finish(settings::Flags::new(flags))
        .ok()?;
    entries.insert(key, Arc::clone(&isa));
    Some(isa)
}
