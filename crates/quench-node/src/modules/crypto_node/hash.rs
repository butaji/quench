//! `Hash`, `Hmac`, `crypto.hash()` and `getHashes()`, and the lazy Transform both streams
//! are (`internal/streams/lazy_transform`).

use super::errors::{self, CryptoError};
use super::{arg, attach, construct_if_called, define_methods, get, put, with_native, Class, Context, Members, Native, Outcome};
use crate::modules::crypto_digest::{Digest, DigestError};
use crate::modules::crypto_digest_error::{self as digest_errors, Operation};
use crate::modules::crypto_digest_state::{HashState, HmacState};
use quench_runtime::{DescriptorFields, RootId, RootedError};
use std::sync::Arc;

/// A Hash or Hmac in progress, and whether `digest()` ended it.
pub(crate) struct Digesting {
    context: DigestContext,
    finalized: bool,
}

enum DigestContext {
    Hash(HashState),
    Hmac(HmacState),
}

impl Digesting {
    fn update(&mut self, bytes: &[u8]) -> Result<bool, DigestError> {
        match &mut self.context {
            DigestContext::Hash(state) => state.update(bytes),
            DigestContext::Hmac(state) => state.update(bytes),
        }
    }

    fn digest(&mut self) -> Result<Vec<u8>, DigestError> {
        match &mut self.context {
            DigestContext::Hash(state) => state.digest(),
            DigestContext::Hmac(state) => state.digest(),
        }
    }
}

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

/// `options.outputLength`, validated as Node's `validateUint32`.
fn output_length(c: &mut Context<'_>, options: RootId) -> Result<Option<usize>, RootedError> {
    let length = super::option(c, options, "outputLength")?;
    if super::is_undefined(c, length) {
        return Ok(None);
    }
    let length = super::integer_argument(c, length, "options.outputLength", 0.0, f64::from(u32::MAX))?;
    Ok(Some(length as usize))
}

/// `new Hash(algorithm[, options])`; `algorithm` may be a Hash to copy.
pub(crate) fn hash_constructor(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    if let Some(instance) = construct_if_called(c, this, Class::Hash, args)? {
        return Ok(instance);
    }
    let (algorithm, options) = (arg(c, args, 0), arg(c, args, 1));
    let copying = with_native(c, algorithm, |native| {
        matches!(native, Native::Hash(Digesting { context: DigestContext::Hash(_), .. }))
    })?;
    let state = match copying {
        Some(true) => {
            let length = output_length(c, options)?;
            let copied = with_native(c, algorithm, |native| match native {
                Native::Hash(Digesting { context: DigestContext::Hash(state), .. }) => Some(state.copy(length)),
                _ => None,
            })?;
            copied.flatten().unwrap_or(Err(DigestError::Unsupported))
        }
        _ => {
            let name = super::string_argument(c, algorithm, "algorithm")?;
            let length = output_length(c, options)?;
            digest_by_name(c, &name).and_then(|digest| HashState::new(digest, length, None, None))
        }
    };
    let state = state.map_err(|error| digest_errors::throw(c, Operation::Hash, error))?;
    attach(c, this, Native::Hash(Digesting { context: DigestContext::Hash(state), finalized: false }))?;
    put(c, this, "_options", options)?;
    Ok(c.undefined())
}

/// `new Hmac(algorithm, key[, options])`.
pub(crate) fn hmac_constructor(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    if let Some(instance) = construct_if_called(c, this, Class::Hmac, args)? {
        return Ok(instance);
    }
    let (algorithm, key, options) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    let name = super::string_argument(c, algorithm, "hmac")?;
    let encoding = string_option(c, options, "encoding")?;
    let key = super::keys::prepare_secret_key(c, key, encoding)?;
    let operation = Operation::Hmac(&name);
    let state = digest_by_name(c, &name).and_then(|digest| HmacState::new(digest, &key));
    let state = state.map_err(|error| digest_errors::throw(c, operation, error))?;
    attach(c, this, Native::Hash(Digesting { context: DigestContext::Hmac(state), finalized: false }))?;
    put(c, this, "_options", options)?;
    Ok(c.undefined())
}

/// Node's `getStringOption`: `options[name]`, which must be a string when present.
pub(crate) fn string_option(c: &mut Context<'_>, options: RootId, name: &str) -> Outcome {
    let value = super::option(c, options, name)?;
    if !super::is_undefined(c, value) {
        super::string_argument(c, value, &format!("options.{name}"))?;
    }
    Ok(value)
}

fn receiver_error(c: &mut Context<'_>) -> RootedError {
    errors::throw(c, CryptoError::PlainType("Illegal invocation"))
}

/// Runs `f` over the receiver's digest state.
fn digesting<R>(c: &mut Context<'_>, this: RootId, f: impl FnOnce(&mut Digesting) -> R) -> Result<R, RootedError> {
    let result = with_native(c, this, |native| match native {
        Native::Hash(state) => Some(f(state)),
        _ => None,
    })?;
    result.flatten().ok_or_else(|| receiver_error(c))
}

/// `hash.update(data[, inputEncoding])`, shared by Hmac.
pub(crate) fn update(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (data, encoding) = (arg(c, args, 0), arg(c, args, 1));
    if digesting(c, this, |state| state.finalized)? {
        return Err(errors::throw(c, CryptoError::HashFinalized));
    }
    let bytes = super::update_data(c, data, encoding)?;
    match digesting(c, this, |state| state.update(&bytes))? {
        Ok(true) => Ok(this),
        Ok(false) => Err(errors::throw(c, CryptoError::HashUpdateFailed)),
        Err(error) => Err(digest_errors::throw(c, Operation::Hash, error)),
    }
}

/// `hash.digest([encoding])`; a finished Hmac digests to the empty string.
pub(crate) fn digest(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let encoding = arg(c, args, 0);
    let encoding = encoding_argument(c, encoding)?;
    let (finalized, is_hmac) = digesting(c, this, |state| (state.finalized, matches!(state.context, DigestContext::Hmac(_))))?;
    if finalized {
        if !is_hmac {
            return Err(errors::throw(c, CryptoError::HashFinalized));
        }
        return super::encode_output(c, Vec::new(), encoding);
    }
    let bytes = digesting(c, this, |state| {
        state.finalized = true;
        state.digest()
    })?;
    let bytes = bytes.map_err(|error| digest_errors::throw(c, Operation::Hash, error))?;
    super::encode_output(c, bytes, encoding)
}

/// `outputEncoding && \`${outputEncoding}\``: truthy encodings as strings.
fn encoding_argument(c: &mut Context<'_>, encoding: RootId) -> Outcome {
    if c.truthy_rooted(encoding)? && !super::is_string(c, encoding) {
        let text = c.to_string(encoding)?;
        return Ok(c.string_rooted(&text));
    }
    Ok(encoding)
}

/// `hash.copy([options])`.
pub(crate) fn copy(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    if digesting(c, this, |state| state.finalized)? {
        return Err(errors::throw(c, CryptoError::HashFinalized));
    }
    let options = arg(c, args, 0);
    let constructor = super::class(c, Class::Hash)?;
    c.construct_rooted(constructor, constructor, &[this, options])
}

/// `_transform(chunk, encoding, callback)`: digest the chunk.
pub(crate) fn transform(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (chunk, encoding, callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    let bytes = super::update_data(c, chunk, encoding)?;
    digesting(c, this, |state| state.update(&bytes))?.map_err(|error| digest_errors::throw(c, Operation::Hash, error))?;
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[])?;
    Ok(undefined)
}

/// `_flush(callback)`: push the digest.
pub(crate) fn flush(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let callback = arg(c, args, 0);
    let bytes = digesting(c, this, |state| state.digest())?.map_err(|error| digest_errors::throw(c, Operation::Hash, error))?;
    let bytes = super::buffer(c, bytes)?;
    super::call_method(c, this, "push", &[bytes])?;
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[])?;
    Ok(undefined)
}

/// `crypto.hash(algorithm, input[, outputEncoding = 'hex'])`.
pub(crate) fn one_shot(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (algorithm, input, encoding) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    let name = super::string_argument(c, algorithm, "algorithm")?;
    if !super::is_string(c, input) && !super::is_view(c, input)? {
        return Err(errors::invalid_type(c, input, "input", super::accepts::DATA));
    }
    let encoding = if super::is_undefined(c, encoding) { c.string_rooted("hex") } else { encoding };
    let normalized = match c.string_text(encoding)? {
        Some(name) if name == "buffer" => None,
        Some(name) if super::canonical_encoding(&name).is_some() => super::canonical_encoding(&name),
        _ => return Err(errors::invalid_value(c, encoding, "outputEncoding", "is invalid")),
    };
    let bytes = match c.string_code_units(input)? {
        Some(units) => crate::modules::buffer_codec::encode_units(&units, "utf8"),
        None => super::buffer_bytes(c, input).unwrap_or_default(),
    };
    let result = digest_by_name(c, &name).and_then(|digest| digest.hash(&bytes, None));
    let bytes = result.map_err(|error| match error {
        DigestError::Unsupported => {
            let message = format!("Digest method {name} is not supported");
            match c.error_rooted(&message) {
                Ok(error) => c.throw(error),
                Err(error) => error,
            }
        }
        other => digest_errors::throw(c, Operation::Hash, other),
    })?;
    match normalized {
        Some(canonical) => {
            let units = crate::modules::buffer_codec::decode_units(&bytes, canonical);
            Ok(c.string_units_rooted(&units))
        }
        None => super::buffer(c, bytes),
    }
}

/// `crypto.getHashes()`: every name a digest fetch accepts, sorted.
pub(crate) fn get_hashes(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let names = super::registered_names(super::Registry::Digests, |name| Digest::fetch_preserving_errors(name).is_some());
    super::string_array(c, &names)
}

// ---- LazyTransform -------------------------------------------------------------------

/// The stream state LazyTransform initializes on first access.
const LAZY_STATES: [&str; 2] = ["_readableState", "_writableState"];

/// `LazyTransform`: a Transform whose stream state is made when first read.
pub(crate) fn lazy_transform(c: &mut Context<'_>, transform: RootId) -> Outcome {
    let constructor = super::subclass(c, "cryptoLazyTransform", "LazyTransform", transform)?;
    let prototype = get(c, constructor, "prototype")?;
    for name in LAZY_STATES {
        let data = c.string_rooted(name);
        let getter = c.host_function_named_with_data(crate::host::shared_vm::operation("cryptoLazyTransformGet"), "get", data)?;
        let setter = c.host_function_named_with_data(crate::host::shared_vm::operation("cryptoLazyTransformSet"), "set", data)?;
        let key = c.string_rooted(name);
        let fields = DescriptorFields {
            get: Some(getter),
            set: Some(setter),
            enumerable: Some(true),
            configurable: Some(true),
            ..DescriptorFields::default()
        };
        c.define_property_fields_rooted(prototype, key, fields)?;
    }
    Ok(constructor)
}

/// `LazyTransform(options)`: keep the options for the deferred Transform.
pub(crate) fn lazy_transform_constructor(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let options = arg(c, args, 0);
    put(c, this, "_options", options)?;
    Ok(c.undefined())
}

/// The lazy state getter: run Transform's constructor over the receiver, then read again.
pub(crate) fn lazy_transform_get(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let name = c.host_function_data()?;
    let options = get(c, this, "_options")?;
    let stream = crate::modules::stream_shared_vm::module(c)?;
    let transform = get(c, stream, "Transform")?;
    c.call_rooted(transform, this, &[options])?;
    let writable = get(c, this, "_writableState")?;
    let no = c.boolean(false);
    put(c, writable, "decodeStrings", no)?;
    c.get_property_rooted(this, name)
}

/// The lazy state setter: an own data property replaces the accessor.
pub(crate) fn lazy_transform_set(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let name = c.host_function_data()?;
    let value = arg(c, args, 0);
    c.define_data_property_rooted(this, name, value, true, true, true)?;
    Ok(c.undefined())
}

pub(crate) fn build(c: &mut Context<'_>, lazy: RootId) -> Result<Vec<(Class, RootId)>, RootedError> {
    let hash = super::subclass(c, "cryptoHashConstructor", "Hash", lazy)?;
    let prototype = get(c, hash, "prototype")?;
    define_methods(
        c,
        prototype,
        &[
            ("copy", "cryptoHashCopy"),
            ("_transform", "cryptoHashTransform"),
            ("_flush", "cryptoHashFlush"),
            ("update", "cryptoHashUpdate"),
            ("digest", "cryptoHashDigest"),
        ],
        Members::Assigned,
    )?;
    let hmac = super::subclass(c, "cryptoHmacConstructor", "Hmac", lazy)?;
    let prototype = get(c, hmac, "prototype")?;
    define_methods(
        c,
        prototype,
        &[
            ("update", "cryptoHashUpdate"),
            ("digest", "cryptoHashDigest"),
            ("_flush", "cryptoHashFlush"),
            ("_transform", "cryptoHashTransform"),
        ],
        Members::Assigned,
    )?;
    Ok(vec![(Class::Hash, hash), (Class::Hmac, hmac)])
}

/// `crypto.createHash(algorithm[, options])`.
pub(crate) fn create_hash(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let constructor = super::class(c, Class::Hash)?;
    let values = [arg(c, args, 0), arg(c, args, 1)];
    c.construct_rooted(constructor, constructor, &values)
}

/// `crypto.createHmac(algorithm, key[, options])`.
pub(crate) fn create_hmac(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let constructor = super::class(c, Class::Hmac)?;
    let values = [arg(c, args, 0), arg(c, args, 1), arg(c, args, 2)];
    c.construct_rooted(constructor, constructor, &values)
}
