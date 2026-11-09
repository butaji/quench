//! Shared digest provider lookup and LazyTransform support for native crypto.
use super::{Context, Outcome};
use crate::modules::crypto_digest::{Digest, DigestError};
use quench_runtime::RootId;
use std::sync::Arc;

/// The digest `name` fetches, memoized per name for the current provider configuration.
pub(crate) fn digest_by_name(c: &mut Context<'_>, name: &str) -> Result<Arc<Digest>, DigestError> {
    let generation = crate::modules::crypto_provider_facts::fips_generation();
    let shared = c.host_mut().shared_state();
    {
        let state = shared.borrow();
        let cache = &state.crypto.digests;
        if cache.generation == generation {
            if let Some(digest) = cache.by_name.get(name) {
                return Ok(digest.clone());
            }
        }
    }
    let digest = Arc::new(Digest::fetch(name)?);
    let mut state = shared.borrow_mut();
    let cache = &mut state.crypto.digests;
    if cache.generation != generation {
        cache.by_name.clear();
        cache.generation = generation;
    }
    cache.by_name.insert(name.to_owned(), digest.clone());
    Ok(digest)
}

/// Digests fetched by name, valid for one provider generation.
#[derive(Default)]
pub(crate) struct DigestCache {
    generation: u64,
    by_name: std::collections::HashMap<String, Arc<Digest>>,
}

/// Node's `getStringOption`: `options[name]`, which must be a string when present.
pub(crate) fn string_option(c: &mut Context<'_>, options: RootId, name: &str) -> Outcome {
    let value = super::option(c, options, name)?;
    if !super::is_undefined(c, value) {
        super::string_argument(c, value, &format!("options.{name}"))?;
    }
    Ok(value)
}

/// `crypto.getHashes()`: every name a digest fetch accepts, sorted.
pub(crate) fn get_hashes(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let names = super::registered_names(super::Registry::Digests, |name| Digest::fetch_preserving_errors(name).is_some());
    super::string_array(c, &names)
}
