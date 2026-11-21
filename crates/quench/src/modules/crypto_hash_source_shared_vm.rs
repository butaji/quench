//! Public Hash/Hmac policy over the canonical native digest binding.
use super::crypto_hash_input_shared_vm as input;
use super::keys::HANDLE;
use super::host_members::leading;
use crate::host::NodeHost;
use quench_runtime::{BuiltinBrand, NativeContext, RootId, RootedError, ValueType};
pub(super) type Context<'a> = NativeContext<'a, NodeHost>;

const STATE: &str = "state";
const FINALIZED: &str = "finalized";
const HASH: &str = "Hash";
const HMAC: &str = "Hmac";
const NATIVE_HASH: &str = "nativeHash";
const NATIVE_HMAC: &str = "nativeHmac";
const ONE_SHOT_DIGEST: &str = "oneShotDigest";
const LAZY_TRANSFORM: &str = "lazyTransform";
const HASH_FINALIZED: &str = "ERR_CRYPTO_HASH_FINALIZED";
const HASH_UPDATE_FAILED: &str = "ERR_CRYPTO_HASH_UPDATE_FAILED";
const SHAKE_WARNED: &str = "shakeWarned";
/// The policy data's private keys of an instance, with their symbols' descriptions.
const INSTANCE_KEYS: [(&str, &str); 3] = [(HANDLE, "kHandle"), (STATE, "kState"), (FINALIZED, "kFinalized")];

/// Where a prototype's method comes from: its own operation, or Hash's prototype.
#[derive(Clone, Copy)]
enum Method {
    Own(&'static str),
    FromHash,
}

/// A public class: its constructor operation, its deprecation warning and its prototype's
/// methods in definition order.
struct Class {
    name: &'static str,
    operation: &'static str,
    deprecation: (&'static str, &'static str),
    methods: &'static [(&'static str, Method)],
}

/// Hash first: Hmac's prototype shares methods of Hash's.
const CLASSES: [Class; 2] = [
    Class {
        name: HASH,
        operation: "cryptoPublicHash",
        deprecation: ("crypto.Hash constructor is deprecated.", "DEP0179"),
        methods: &[
            ("copy", Method::Own("cryptoPublicHashCopy")),
            ("_transform", Method::Own("cryptoPublicHashTransform")),
            ("_flush", Method::Own("cryptoPublicHashFlush")),
            ("update", Method::Own("cryptoPublicHashUpdate")),
            ("digest", Method::Own("cryptoPublicHashDigest")),
        ],
    },
    Class {
        name: HMAC,
        operation: "cryptoPublicHmac",
        deprecation: ("crypto.Hmac constructor is deprecated.", "DEP0181"),
        methods: &[
            ("update", Method::FromHash),
            ("digest", Method::Own("cryptoPublicHmacDigest")),
            ("_flush", Method::Own("cryptoPublicHmacFlush")),
            ("_transform", Method::FromHash),
        ],
    },
];
const FACTORIES: [(&str, &str); 2] = [("createHash", "cryptoPublicCreateHash"), ("createHmac", "cryptoPublicCreateHmac")];

fn arg(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| c.undefined())
}
pub(super) fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}
fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    c.assign_named_property_rooted(object, name, value)
}
pub(super) fn undefined(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_undefined())
}
pub(super) fn nullish(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_null() || value.is_undefined())
}
fn invoke(c: &mut Context<'_>, object: RootId, method: &str, args: &[RootId]) -> Result<RootId, RootedError> {
    let function = get(c, object, method)?;
    c.call_rooted(function, object, args)
}
fn crypto_error(c: &mut Context<'_>, code: &'static str, message: &str) -> Result<RootedError, RootedError> {
    let error = c.error_rooted(message)?;
    super::node_errors_shared_vm::decorate_error(c, error, code)?;
    Ok(c.throw(error))
}
fn native_handle(c: &mut Context<'_>, data: RootId, receiver: RootId) -> Result<RootId, RootedError> {
    let key = get(c, data, HANDLE)?;
    c.get_property_rooted(receiver, key)
}
fn state(c: &mut Context<'_>, data: RootId, receiver: RootId) -> Result<(RootId, RootId), RootedError> {
    let key = get(c, data, STATE)?;
    let state = c.get_property_rooted(receiver, key)?;
    let finalized = get(c, data, FINALIZED)?;
    Ok((state, finalized))
}
fn ensure_active(c: &mut Context<'_>, data: RootId, receiver: RootId) -> Result<(), RootedError> {
    let (state, finalized) = state(c, data, receiver)?;
    let value = c.get_property_rooted(state, finalized)?;
    if c.truthy_rooted(value)? {
        return Err(crypto_error(c, HASH_FINALIZED, "Digest already called")?);
    }
    Ok(())
}
fn finalize(c: &mut Context<'_>, state: RootId, finalized: RootId) -> Result<(), RootedError> {
    let yes = c.boolean(true);
    c.assign_property_rooted(state, finalized, yes)
}
fn initialize_instance(
    c: &mut Context<'_>,
    data: RootId,
    receiver: RootId,
    handle: RootId,
    options: RootId,
    implicit_length: Option<RootId>,
) -> Result<RootId, RootedError> {
    let key = get(c, data, HANDLE)?;
    c.assign_property_rooted(receiver, key, handle)?;
    let state = c.object_rooted()?;
    let key = get(c, data, FINALIZED)?;
    let no = c.boolean(false);
    c.define_data_property_rooted(state, key, no, true, true, true)?;
    let key = get(c, data, STATE)?;
    c.assign_property_rooted(receiver, key, state)?;
    if let Some(algorithm) = implicit_length {
        warn_implicit_length(c, data, algorithm)?;
    }
    let lazy = get(c, data, LAZY_TRANSFORM)?;
    c.call_rooted(lazy, receiver, &[options])?;
    Ok(receiver)
}
fn warn_implicit_length(c: &mut Context<'_>, data: RootId, algorithm: RootId) -> Result<(), RootedError> {
    let name = c.string_text(algorithm)?.expect("a new hash validated its algorithm");
    let name = name.to_lowercase().replacen('-', "", 1);
    if !matches!(name.as_str(), "shake128" | "shake256") {
        return Ok(());
    }
    let warned = get(c, data, SHAKE_WARNED)?;
    if c.truthy_rooted(warned)? {
        return Ok(());
    }
    let yes = c.boolean(true);
    put(c, data, SHAKE_WARNED, yes)?;
    let message = c.string_rooted("Creating SHAKE128/256 digests without an explicit options.outputLength is deprecated.");
    let code = c.string_rooted("DEP0198");
    let undefined = c.undefined();
    super::util_module_shared_vm::emit_deprecation(c, message, code, undefined)
}

/// Builds this policy with the module's canonical LazyTransform constructor.
pub(crate) fn install(c: &mut Context<'_>, public: RootId, lazy: RootId) -> Result<(RootId, RootId), RootedError> {
    let data = policy_data(c, public, lazy)?;
    let operation = crate::host::shared_vm::operation;
    let one_shot = c.host_function_named_with_data(operation("cryptoPublicOneShot"), "hash", data)?;
    put(c, public, "hash", one_shot)?;
    let parent = (lazy, get(c, lazy, "prototype")?);
    let [hash, hmac] = &CLASSES;
    let hash_prototype = install_class(c, public, data, parent, hash)?;
    let hmac_prototype = install_class(c, public, data, parent, hmac)?;
    install_methods(c, data, hash_prototype, hash_prototype, hash.methods)?;
    install_methods(c, data, hmac_prototype, hash_prototype, hmac.methods)?;
    for (name, callback) in FACTORIES {
        let function = c.host_function_named_with_data(operation(callback), name, data)?;
        put(c, public, name, function)?;
    }
    Ok((get(c, data, HASH)?, get(c, data, HMAC)?))
}

/// The policy's shared function data: the native binding's classes and one-shot digest,
/// the instances' private keys and the LazyTransform constructor.
fn policy_data(c: &mut Context<'_>, public: RootId, lazy: RootId) -> Result<RootId, RootedError> {
    let data = c.null_object_rooted()?;
    let native = super::crypto_hash_shared_vm::binding(c, public, &[])?;
    for (name, export) in [(ONE_SHOT_DIGEST, ONE_SHOT_DIGEST), (NATIVE_HASH, HASH), (NATIVE_HMAC, HMAC)] {
        let value = get(c, native, export)?;
        put(c, data, name, value)?;
    }
    for (name, description) in INSTANCE_KEYS {
        let symbol = c.symbol_rooted(Some(description));
        put(c, data, name, symbol)?;
    }
    put(c, data, LAZY_TRANSFORM, lazy)?;
    Ok(data)
}

/// A class's constructor, inheriting from LazyTransform and exported behind its deprecation
/// warning; returns its prototype.
fn install_class(
    c: &mut Context<'_>,
    public: RootId,
    data: RootId,
    (lazy, lazy_prototype): (RootId, RootId),
    class: &Class,
) -> Result<RootId, RootedError> {
    let operation = crate::host::shared_vm::operation(class.operation);
    let constructor = c.host_constructor_named_with_data(operation, class.name, data)?;
    let prototype = get(c, constructor, "prototype")?;
    c.set_prototype_rooted(constructor, lazy)?;
    c.set_prototype_rooted(prototype, lazy_prototype)?;
    put(c, data, class.name, constructor)?;
    let (message, code) = class.deprecation;
    let message = c.string_rooted(message);
    let code = c.string_rooted(code);
    let exported = super::util_module_shared_vm::deprecated_wrapper(c, constructor, message, code)?;
    put(c, public, class.name, exported)?;
    Ok(prototype)
}

fn install_methods(
    c: &mut Context<'_>,
    data: RootId,
    prototype: RootId,
    hash_prototype: RootId,
    methods: &[(&str, Method)],
) -> Result<(), RootedError> {
    for &(name, method) in methods {
        let function = match method {
            Method::Own(callback) => c.host_function_named_with_data(crate::host::shared_vm::operation(callback), name, data)?,
            Method::FromHash => get(c, hash_prototype, name)?,
        };
        put(c, prototype, name, function)?;
    }
    Ok(())
}

fn construct_class(c: &mut Context<'_>, name: &str, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let constructor = get(c, data, name)?;
    c.construct_rooted(constructor, constructor, args)
}
pub(crate) fn create_hash(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    construct_class(c, HASH, args)
}
pub(crate) fn create_hmac(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    construct_class(c, HMAC, args)
}
pub(crate) fn hash(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    if c.new_target_rooted().is_none() {
        return create_hash(c, this, args);
    }
    let data = c.host_function_data()?;
    let ([algorithm, options], _) = leading(c, args);
    let native = get(c, data, NATIVE_HASH)?;
    let copying = c.instance_of_rooted(algorithm, native)?;
    if !copying {
        input::validate_string(c, algorithm, "algorithm")?;
    }
    let length = input::output_length(c, options)?;
    let mut parameters = vec![algorithm, length];
    if !copying && !nullish(c, options) {
        let [function_name, customization] = input::parameters(c, options)?;
        if !undefined(c, function_name) || !undefined(c, customization) {
            let missing = c.undefined();
            parameters.extend_from_slice(&[missing, missing, function_name, customization]);
        }
    }
    let handle = c.construct_rooted(native, native, &parameters)?;
    let implicit_length = (!copying && undefined(c, length)).then_some(algorithm);
    initialize_instance(c, data, this, handle, options, implicit_length)
}
pub(crate) fn hmac(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let constructor = get(c, data, HMAC)?;
    if !c.instance_of_rooted(this, constructor)? {
        return create_hmac(c, this, args);
    }
    let ([algorithm, key, options], _) = leading(c, args);
    input::validate_string(c, algorithm, "hmac")?;
    let encoding = if c.truthy_rooted(options)? { get(c, options, "encoding")? } else { c.undefined() };
    if !nullish(c, encoding) {
        input::validate_string(c, encoding, "options.encoding")?;
    }
    let key = hmac_key(c, key, encoding)?;
    let native = get(c, data, NATIVE_HMAC)?;
    let handle = c.construct_rooted(native, native, &[])?;
    invoke(c, handle, "init", &[algorithm, key])?;
    initialize_instance(c, data, this, handle, options, None)?;
    Ok(c.undefined())
}
/// An Hmac key's bytes: a buffer source as is, otherwise a secret KeyObject or CryptoKey.
fn hmac_key(c: &mut Context<'_>, key: RootId, encoding: RootId) -> Result<RootId, RootedError> {
    if c.value_type_rooted(key) == Some(ValueType::String)
        || c.has_builtin_brand_rooted(key, BuiltinBrand::AnyArrayBuffer)?
        || c.has_builtin_brand_rooted(key, BuiltinBrand::ArrayBufferView)?
    {
        return input::buffer_source(c, key, "key", Some(encoding));
    }
    let material = super::crypto_node::keys::prepare_secret_key(c, key, encoding)?;
    super::buffer_shared_vm::from_bytes(c, &material)
}
pub(crate) fn copy(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    ensure_active(c, data, this)?;
    let handle = native_handle(c, data, this)?;
    let options = arg(c, args, 0);
    let constructor = get(c, data, HASH)?;
    c.construct_rooted(constructor, constructor, &[handle, options])
}
pub(crate) fn update(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    ensure_active(c, data, this)?;
    let ([input, encoding], _) = leading(c, args);
    if c.value_type_rooted(input) == Some(ValueType::String) {
        input::validate_hex_length(c, input, encoding)?;
    } else {
        input::validate_data(c, input, "data")?;
    }
    let handle = native_handle(c, data, this)?;
    let updated = invoke(c, handle, "update", &[input, encoding])?;
    if !c.truthy_rooted(updated)? {
        return Err(crypto_error(c, HASH_UPDATE_FAILED, "Hash update failed")?);
    }
    Ok(this)
}
fn digest<const IS_HMAC: bool>(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let (state, finalized) = state(c, data, this)?;
    let done = c.get_property_rooted(state, finalized)?;
    let output = arg(c, args, 0);
    if c.truthy_rooted(done)? {
        if !IS_HMAC {
            return Err(crypto_error(c, HASH_FINALIZED, "Digest already called")?);
        }
        let empty = super::buffer_shared_vm::from_bytes(c, &[])?;
        if c.truthy_rooted(output)? && c.string_text(output)?.as_deref() != Some("buffer") {
            return invoke(c, empty, "toString", &[output]);
        }
        return Ok(empty);
    }
    let output = if c.truthy_rooted(output)? {
        let text = c.to_string_code_units(output)?;
        c.string_units_rooted(&text)
    } else {
        output
    };
    let handle = native_handle(c, data, this)?;
    let result = invoke(c, handle, "digest", &[output])?;
    finalize(c, state, finalized)?;
    Ok(result)
}
pub(crate) fn hash_digest(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    digest::<false>(c, this, args)
}
pub(crate) fn hmac_digest(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    digest::<true>(c, this, args)
}
pub(crate) fn transform(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let handle = native_handle(c, data, this)?;
    let ([chunk, encoding, callback], _) = leading(c, args);
    let result = invoke(c, handle, "update", &[chunk, encoding])?;
    let undefined = c.undefined();
    if c.truthy_rooted(result)? {
        c.call_rooted(callback, undefined, &[])?;
        return Ok(c.undefined());
    }
    let error = crypto_error(c, HASH_UPDATE_FAILED, "Hash update failed")?;
    c.call_rooted(callback, undefined, &[error.exception.expect("coded error has an exception")])
}
fn flush<const IS_HMAC: bool>(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let handle = native_handle(c, data, this)?;
    let bytes = invoke(c, handle, "digest", &[])?;
    invoke(c, this, "push", &[bytes])?;
    if IS_HMAC {
        let (state, finalized) = state(c, data, this)?;
        finalize(c, state, finalized)?;
    }
    let callback = arg(c, args, 0);
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[])?;
    Ok(c.undefined())
}
pub(crate) fn hash_flush(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    flush::<false>(c, this, args)
}
pub(crate) fn hmac_flush(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    flush::<true>(c, this, args)
}

/// `crypto.hash`: validation and normalization precede the shared native one-shot digest.
pub(crate) fn one_shot(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let ([algorithm, data_input, options], _) = leading(c, args);
    input::validate_string(c, algorithm, "algorithm")?;
    input::validate_data(c, data_input, "input")?;
    let options = input::one_shot_options(c, options)?;
    let encoding = input::output_encoding(c, options.encoding)?;
    let length = input::checked_output_length(c, options.length, "outputLength")?;
    if undefined(c, length) {
        warn_implicit_length(c, data, algorithm)?;
    }
    let native = get(c, data, ONE_SHOT_DIGEST)?;
    let missing = c.undefined();
    let [function_name, customization] = options.parameters;
    // The native one-shot digest's positional protocol (`crypto_hash_shared_vm::one_shot`):
    // no cached algorithm id or alias cache, and no input encoding for a validated input.
    let native_args = [algorithm, missing, missing, data_input, encoding, missing, length, function_name, customization];
    c.call_rooted(native, missing, &native_args)
}
