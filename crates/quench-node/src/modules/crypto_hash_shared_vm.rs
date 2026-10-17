//! Native stateful hash handles used by the unmodified SDK hash.js policy.
//! Only weak VM identities are stored here. Fetched Md/context owners never
//! point into guest memory, and input views are consumed during each update.
use super::{
    crypto_digest::{Digest, DigestError},
    crypto_digest_error::{self as errors, Operation},
    crypto_digest_state::{HashState, HmacState},
};
use crate::host::NodeHost;
use quench_runtime::{HostWeakRoot, NativeContext, RootId, RootedError, Runtime};
use std::sync::Arc;
type Context<'a> = NativeContext<'a, NodeHost>;
#[derive(Default)]
pub(crate) struct State {
    handles: Vec<Handle>,
}
struct Handle {
    identity: HostWeakRoot,
    value: Value,
}
enum Value {
    Hash(HashState),
    Hmac(Option<HmacState>),
}
fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}
fn fail(c: &mut Context<'_>, message: &str) -> RootedError {
    match c.type_error_rooted(message) {
        Ok(root) => c.throw(root),
        Err(error) => error,
    }
}
fn string(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    c.string_text(value)?
        .ok_or_else(|| fail(c, "Native hash algorithm must be a string"))
}
fn length(c: &mut Context<'_>, value: Option<RootId>) -> Result<Option<usize>, RootedError> {
    let Some(value) = value
        .and_then(|root| c.rooted_value(root))
        .filter(|value| !value.is_undefined())
    else {
        return Ok(None);
    };
    value
        .as_number()
        .filter(|n| n.is_finite() && *n >= 0.0 && *n <= u32::MAX as f64 && n.fract() == 0.0)
        .map(|n| Some(n as usize))
        .ok_or_else(|| fail(c, "Native hash output length must be uint32"))
}
fn options(
    c: &mut Context<'_>,
    args: &[RootId],
    first: usize,
) -> Result<(Option<Vec<u8>>, Option<Vec<u8>>), RootedError> {
    let mut result = [None, None];
    for (index, output) in result.iter_mut().enumerate() {
        if let Some(root) = args.get(first + index).copied() {
            if !c
                .rooted_value(root)
                .is_some_and(|value| value.is_undefined())
            {
                *output = Some(
                    c.bytes_prefix_rooted(root, usize::MAX)
                        .ok_or_else(|| fail(c, "Native hash customization must be a byte view"))?,
                );
            }
        }
    }
    let [function_name, customization] = result;
    Ok((function_name, customization))
}
pub(crate) fn input(
    c: &mut Context<'_>,
    value: RootId,
    encoding: Option<RootId>,
) -> Result<Vec<u8>, RootedError> {
    if let Some(units) = c.string_code_units(value)? {
        let encoding = encoding
            .and_then(|root| c.string_text(root).ok().flatten())
            .and_then(|text| super::buffer_codec::canonical_encoding(&text))
            .unwrap_or("utf8");
        return Ok(super::buffer_codec::encode_units(&units, encoding));
    }
    c.bytes_prefix_rooted(value, usize::MAX)
        .ok_or_else(|| fail(c, "Native hash input must be a string or byte view"))
}
fn output(
    c: &mut Context<'_>,
    bytes: &[u8],
    encoding: Option<RootId>,
) -> Result<RootId, RootedError> {
    let encoding = encoding
        .and_then(|root| c.string_text(root).ok().flatten())
        .and_then(|text| super::buffer_codec::canonical_encoding(&text));
    if let Some(encoding) = encoding {
        let units = super::buffer_codec::decode_units_limited(
            bytes,
            encoding,
            Context::MAX_STRING_CODE_UNITS,
        )
        .ok_or_else(|| {
            match c
                .range_error_rooted("Cannot create a string longer than the maximum string length")
            {
                Ok(root) => c.throw(root),
                Err(error) => error,
            }
        })?;
        return Ok(c.string_units_rooted(&units));
    }
    super::buffer_shared_vm::from_bytes(c, bytes)
}
fn index(c: &mut Context<'_>, receiver: RootId) -> Result<usize, RootedError> {
    let shared = c.host_mut().shared_state();
    let identities = shared
        .borrow()
        .crypto_hashes
        .handles
        .iter()
        .map(|handle| handle.identity)
        .collect::<Vec<_>>();
    for (index, identity) in identities.into_iter().enumerate() {
        if let Some(value) = c.resolve_host_weak_root(identity) {
            if c.strict_equal_rooted(value, receiver)? {
                return Ok(index);
            }
        }
    }
    Err(fail(c, "Invalid native hash receiver"))
}
fn register(c: &mut Context<'_>, receiver: RootId, value: Value) -> Result<RootId, RootedError> {
    let shared = c.host_mut().shared_state();
    let dead = shared
        .borrow()
        .crypto_hashes
        .handles
        .iter()
        .enumerate()
        .filter_map(|(index, handle)| {
            c.resolve_host_weak_root(handle.identity)
                .is_none()
                .then_some(index)
        })
        .collect::<Vec<_>>();
    for index in dead.into_iter().rev() {
        let handle = shared.borrow_mut().crypto_hashes.handles.remove(index);
        c.release_host_weak_root(handle.identity);
    }
    let identity = c.host_weak_root(receiver)?;
    shared
        .borrow_mut()
        .crypto_hashes
        .handles
        .push(Handle { identity, value });
    Ok(receiver)
}
fn resolve(
    c: &mut Context<'_>,
    name: &str,
    cached_id: Option<RootId>,
) -> Result<(Arc<Digest>, i32), DigestError> {
    let shared = c.host_mut().shared_state();
    let id = cached_id
        .and_then(|root| c.rooted_value(root))
        .and_then(|value| value.as_number())
        .filter(|n| n.fract() == 0.0 && *n >= i32::MIN as f64 && *n <= i32::MAX as f64)
        .map(|n| n as i32);
    let mut state = shared.borrow_mut();
    state.crypto_secret_jobs.digests.resolve_cached(
        name,
        id,
        super::crypto_provider_shared_vm::fips_generation,
    )
}
fn cache(
    c: &mut Context<'_>,
    name: &str,
    id: i32,
    cache: Option<RootId>,
) -> Result<(), RootedError> {
    if let Some(cache) = cache {
        let key = c.string_rooted(name);
        let value = c.number(f64::from(id));
        if !c.set_property_rooted(cache, key, value, cache)? {
            return Err(fail(c, "Digest cache rejected native alias"));
        }
    }
    Ok(())
}
pub(crate) fn create_hash(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(algorithm) = args.first().copied() else {
        return Err(fail(c, "Native hash algorithm missing"));
    };
    let output_length = length(c, args.get(1).copied())?;
    let state = if c.is_object_rooted(algorithm)? {
        let index = index(c, algorithm)?;
        let shared = c.host_mut().shared_state();
        let result = match &shared.borrow().crypto_hashes.handles[index].value {
            Value::Hash(state) => state.copy(output_length),
            _ => return Err(fail(c, "Invalid native hash copy source")),
        };
        result.map_err(|error| errors::throw(c, Operation::Hash, error))?
    } else {
        let name = string(c, algorithm)?;
        let (digest, id) = resolve(c, &name, args.get(2).copied())
            .map_err(|error| errors::throw(c, Operation::Hash, error))?;
        cache(c, &name, id, args.get(3).copied())?;
        let (function_name, customization) = options(c, args, 4)?;
        HashState::new(
            digest,
            output_length,
            function_name.as_deref(),
            customization.as_deref(),
        )
        .map_err(|error| errors::throw(c, Operation::Hash, error))?
    };
    register(c, receiver, Value::Hash(state))
}
pub(crate) fn create_hmac(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    register(c, receiver, Value::Hmac(None))
}
pub(crate) fn init_hmac(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let index = index(c, receiver)?;
    let missing = c.undefined();
    let name = string(c, args.first().copied().unwrap_or(missing))?;
    let key =
        super::crypto_key_shared_vm::secret_bytes(c, args.get(1).copied().unwrap_or(missing))?;
    let digest = Digest::fetch(&name)
        .map(Arc::new)
        .map_err(|error| errors::throw(c, Operation::Hmac(&name), error))?;
    let result = HmacState::new(digest, &key);
    let state = result.map_err(|error| errors::throw(c, Operation::Hmac(&name), error))?;
    let shared = c.host_mut().shared_state();
    let mut state_ref = shared.borrow_mut();
    let handle = &mut state_ref.crypto_hashes.handles[index];
    if !matches!(handle.value, Value::Hmac(_)) {
        drop(state_ref);
        return Err(fail(c, "Invalid native Hmac receiver"));
    }
    handle.value = Value::Hmac(Some(state));
    Ok(c.undefined())
}
pub(crate) fn update(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let bytes = input(
        c,
        args.first().copied().unwrap_or(missing),
        args.get(1).copied(),
    )?;
    if bytes.len() > i32::MAX as usize {
        let root = c.range_error_rooted("data is too long")?;
        return Err(c.throw(root));
    }
    let index = index(c, receiver)?;
    let shared = c.host_mut().shared_state();
    let result = match &mut shared.borrow_mut().crypto_hashes.handles[index].value {
        Value::Hash(state) => state.update(&bytes),
        Value::Hmac(Some(state)) => state.update(&bytes),
        Value::Hmac(None) => Ok(false),
    };
    let result = result.map_err(|error| errors::throw(c, Operation::Hash, error))?;
    Ok(c.boolean(result))
}
pub(crate) fn digest(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let index = index(c, receiver)?;
    let shared = c.host_mut().shared_state();
    let result = match &mut shared.borrow_mut().crypto_hashes.handles[index].value {
        Value::Hash(state) => state.digest(),
        Value::Hmac(Some(state)) => state.digest(),
        Value::Hmac(None) => Ok(Vec::new()),
    };
    let bytes = result.map_err(|error| errors::throw(c, Operation::Hash, error))?;
    output(c, &bytes, args.first().copied())
}
pub(crate) fn one_shot(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let arg = |i: usize| args.get(i).copied().unwrap_or(missing);
    let name = string(c, arg(0))?;
    let (digest, id) =
        resolve(c, &name, Some(arg(1))).map_err(|error| one_shot_error(c, &name, error))?;
    cache(c, &name, id, Some(arg(2)))?;
    let input = input(c, arg(3), None)?;
    let output_length = length(c, Some(arg(6)))?;
    let (function_name, customization) = options(c, args, 7)?;
    let mut state = HashState::new(
        digest,
        output_length,
        function_name.as_deref(),
        customization.as_deref(),
    )
    .map_err(|error| one_shot_error(c, &name, error))?;
    state
        .update(&input)
        .map_err(|error| one_shot_error(c, &name, error))?;
    let bytes = state
        .digest()
        .map_err(|error| one_shot_error(c, &name, error))?;
    output(c, &bytes, Some(arg(4)))
}
fn one_shot_error(c: &mut Context<'_>, name: &str, error: DigestError) -> RootedError {
    let message = match error {
        DigestError::Unsupported => format!("Digest method {name} is not supported"),
        DigestError::InvalidLength { actual, .. } => {
            format!("Output length {actual} is invalid for {name}, which does not support XOF")
        }
        other => return errors::throw(c, Operation::Hash, other),
    };
    match c.error_rooted(&message) {
        Ok(root) => c.throw(root),
        Err(error) => error,
    }
}
pub(crate) fn install(c: &mut Context<'_>, binding: RootId) -> Result<(), RootedError> {
    for (name, operation, methods) in [
        (
            "Hash",
            "cryptoNativeHashNew",
            &[
                ("update", "cryptoNativeHashUpdate"),
                ("digest", "cryptoNativeHashDigest"),
            ][..],
        ),
        (
            "Hmac",
            "cryptoNativeHmacNew",
            &[
                ("init", "cryptoNativeHmacInit"),
                ("update", "cryptoNativeHashUpdate"),
                ("digest", "cryptoNativeHashDigest"),
            ][..],
        ),
    ] {
        let constructor = c.host_constructor(crate::host::shared_vm::operation(operation))?;
        let key = c.string_rooted("name");
        let text = c.string_rooted(name);
        c.define_data_property_rooted(constructor, key, text, false, false, true)?;
        let prototype = c.object_rooted()?;
        for (name, operation) in methods {
            let function =
                c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
            put(c, prototype, name, function)?;
        }
        let key = c.string_rooted("constructor");
        c.define_data_property_rooted(prototype, key, constructor, true, false, true)?;
        let key = c.string_rooted("prototype");
        c.define_data_property_rooted(constructor, key, prototype, true, false, false)?;
        put(c, binding, name, constructor)?;
    }
    let function = c.host_function_named(
        crate::host::shared_vm::operation("cryptoNativeOneShotDigest"),
        "oneShotDigest",
    )?;
    put(c, binding, "oneShotDigest", function)
}
pub(crate) fn prune(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let dead = shared
        .borrow()
        .crypto_hashes
        .handles
        .iter()
        .enumerate()
        .filter_map(|(index, handle)| {
            (!runtime.host_weak_root_is_live(handle.identity)).then_some(index)
        })
        .collect::<Vec<_>>();
    for index in dead.into_iter().rev() {
        let handle = shared.borrow_mut().crypto_hashes.handles.remove(index);
        runtime.release_host_weak_root(handle.identity);
    }
}
pub(crate) fn cleanup(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let handles = std::mem::take(&mut shared.borrow_mut().crypto_hashes.handles);
    for handle in handles {
        runtime.release_host_weak_root(handle.identity);
    }
}
