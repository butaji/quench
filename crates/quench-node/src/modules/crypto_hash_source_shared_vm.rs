//! Public Hash/Hmac policy over the canonical native digest binding.
use crate::host::NodeHost;
use quench_runtime::{BuiltinBrand, NativeContext, RootId, RootedError, ValueType};
type Context<'a> = NativeContext<'a, NodeHost>;

const HANDLE: &str = "handle";
const STATE: &str = "state";
const FINALIZED: &str = "finalized";
const HASH: &str = "Hash";
const HMAC: &str = "Hmac";
const NATIVE_HASH: &str = "nativeHash";
const NATIVE_HMAC: &str = "nativeHmac";
const LAZY_TRANSFORM: &str = "lazyTransform";
const HASH_FINALIZED: &str = "ERR_CRYPTO_HASH_FINALIZED";
const HASH_UPDATE_FAILED: &str = "ERR_CRYPTO_HASH_UPDATE_FAILED";
const SHAKE_WARNED: &str = "shakeWarned";

fn arg(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| c.undefined())
}
fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}
fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    c.assign_named_property_rooted(object, name, value)
}
fn undefined(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value)
        .is_some_and(|value| value.is_undefined())
}
fn nullish(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value)
        .is_some_and(|value| value.is_null() || value.is_undefined())
}
fn invoke(
    c: &mut Context<'_>,
    object: RootId,
    method: &str,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let function = get(c, object, method)?;
    c.call_rooted(function, object, args)
}
fn crypto_error(
    c: &mut Context<'_>,
    code: &'static str,
    message: &str,
) -> Result<RootedError, RootedError> {
    let error = c.error_rooted(message)?;
    super::node_errors_shared_vm::decorate_error(c, error, code)?;
    Ok(c.throw(error))
}
fn validate_string(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    if c.value_type_rooted(value) == Some(ValueType::String) {
        return Ok(());
    }
    let error = super::util_shared_vm::argument_type_error(c, value, name, "string")?;
    if let Some(exception) = error.exception {
        super::node_errors_shared_vm::hide_type_error(c, exception, "ERR_INVALID_ARG_TYPE")?;
    }
    Err(error)
}
fn output_length(c: &mut Context<'_>, options: RootId) -> Result<RootId, RootedError> {
    if c.value_type_rooted(options) != Some(ValueType::Object) || nullish(c, options) {
        return Ok(c.undefined());
    }
    let length = get(c, options, "outputLength")?;
    if undefined(c, length) {
        return Ok(length);
    }
    let maximum = c.number(f64::from(u32::MAX));
    match super::util_shared_vm::validate_integer(c, length, "options.outputLength", 0.0, maximum) {
        Ok(value) => Ok(c.number(value + 0.0)),
        Err(error) => {
            if let Some(exception) = error.exception {
                if c.rooted_value(length)
                    .is_some_and(|value| value.as_number().is_some())
                {
                    super::node_errors_shared_vm::hide_range_error(
                        c,
                        exception,
                        "ERR_OUT_OF_RANGE",
                    )?;
                } else {
                    super::node_errors_shared_vm::hide_type_error(
                        c,
                        exception,
                        "ERR_INVALID_ARG_TYPE",
                    )?;
                }
            }
            Err(error)
        }
    }
}
fn native_handle(
    c: &mut Context<'_>,
    data: RootId,
    receiver: RootId,
) -> Result<RootId, RootedError> {
    let key = get(c, data, HANDLE)?;
    c.get_property_rooted(receiver, key)
}
fn state(
    c: &mut Context<'_>,
    data: RootId,
    receiver: RootId,
) -> Result<(RootId, RootId), RootedError> {
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
        let name = c
            .string_text(algorithm)?
            .expect("a new hash validated its algorithm");
        let name = name.to_lowercase().replacen('-', "", 1);
        if matches!(name.as_str(), "shake128" | "shake256") {
            let warned = get(c, data, SHAKE_WARNED)?;
            if !c.truthy_rooted(warned)? {
                let yes = c.boolean(true);
                put(c, data, SHAKE_WARNED, yes)?;
                let message = c.string_rooted("Creating SHAKE128/256 digests without an explicit options.outputLength is deprecated.");
                let code = c.string_rooted("DEP0198");
                let undefined = c.undefined();
                super::util_deprecate_shared_vm::emit(c, message, code, undefined)?;
            }
        }
    }
    let lazy = get(c, data, LAZY_TRANSFORM)?;
    c.call_rooted(lazy, receiver, &[options])?;
    Ok(receiver)
}

/// Builds this policy with the caller's canonical Transform constructor.
pub(crate) fn install(
    c: &mut Context<'_>,
    public: RootId,
    transform: RootId,
) -> Result<(), RootedError> {
    let data = c.null_object_rooted()?;
    let native = super::crypto_hash_shared_vm::binding(c, public, &[])?;
    for (name, export) in [(NATIVE_HASH, HASH), (NATIVE_HMAC, HMAC)] {
        let value = get(c, native, export)?;
        put(c, data, name, value)?;
    }
    for (name, description) in [
        (HANDLE, "kHandle"),
        (STATE, "kState"),
        (FINALIZED, "kFinalized"),
    ] {
        let symbol = c.symbol_rooted(Some(description));
        put(c, data, name, symbol)?;
    }
    let lazy = super::crypto_lazy_transform::constructor(c, transform)?;
    put(c, data, LAZY_TRANSFORM, lazy)?;
    let lazy_prototype = get(c, lazy, "prototype")?;
    let operation = crate::host::shared_vm::operation;
    for (name, callback) in [(HASH, "cryptoPublicHash"), (HMAC, "cryptoPublicHmac")] {
        let constructor = c.host_constructor_named_with_data(operation(callback), name, data)?;
        let prototype = get(c, constructor, "prototype")?;
        c.set_prototype_rooted(constructor, lazy)?;
        c.set_prototype_rooted(prototype, lazy_prototype)?;
        put(c, data, name, constructor)?;
        let (message, code) = if name == HASH {
            ("crypto.Hash constructor is deprecated.", "DEP0179")
        } else {
            ("crypto.Hmac constructor is deprecated.", "DEP0181")
        };
        let exported = super::util_deprecate_shared_vm::wrap(c, constructor, message, code)?;
        put(c, public, name, exported)?;
    }
    let hash = get(c, data, HASH)?;
    let hash_prototype = get(c, hash, "prototype")?;
    let hmac = get(c, data, HMAC)?;
    let hmac_prototype = get(c, hmac, "prototype")?;
    for (prototype, methods) in [
        (
            hash_prototype,
            &[
                ("copy", "cryptoPublicHashCopy"),
                ("_transform", "cryptoPublicHashTransform"),
                ("_flush", "cryptoPublicHashFlush"),
                ("update", "cryptoPublicHashUpdate"),
                ("digest", "cryptoPublicHashDigest"),
            ][..],
        ),
        (
            hmac_prototype,
            &[
                ("digest", "cryptoPublicHmacDigest"),
                ("_flush", "cryptoPublicHmacFlush"),
            ][..],
        ),
    ] {
        if prototype == hmac_prototype {
            let update = get(c, hash_prototype, "update")?;
            put(c, hmac_prototype, "update", update)?;
        }
        for &(name, callback) in methods {
            let function = c.host_function_named_with_data(operation(callback), name, data)?;
            put(c, prototype, name, function)?;
        }
    }
    let transform_method = get(c, hash_prototype, "_transform")?;
    put(c, hmac_prototype, "_transform", transform_method)?;
    for (name, callback) in [
        ("createHash", "cryptoPublicCreateHash"),
        ("createHmac", "cryptoPublicCreateHmac"),
    ] {
        let function = c.host_function_named_with_data(operation(callback), name, data)?;
        put(c, public, name, function)?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn install_binding(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let public = arg(c, args, 0);
    let transform = arg(c, args, 1);
    install(c, public, transform)?;
    Ok(public)
}
pub(crate) fn create_hash(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let constructor = get(c, data, HASH)?;
    c.construct_rooted(constructor, constructor, args)
}
pub(crate) fn create_hmac(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let constructor = get(c, data, HMAC)?;
    c.construct_rooted(constructor, constructor, args)
}
pub(crate) fn hash(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    if c.new_target_rooted().is_none() {
        return create_hash(c, this, args);
    }
    let data = c.host_function_data()?;
    let algorithm = arg(c, args, 0);
    let options = arg(c, args, 1);
    let native = get(c, data, NATIVE_HASH)?;
    let copying = c.instance_of_rooted(algorithm, native)?;
    if !copying {
        validate_string(c, algorithm, "algorithm")?;
    }
    let length = output_length(c, options)?;
    let mut parameters = vec![algorithm, length];
    if !copying && !nullish(c, options) {
        let function_name = get(c, options, "functionName")?;
        let function_name = parameter(c, function_name, "options.functionName")?;
        let customization = get(c, options, "customization")?;
        let customization = parameter(c, customization, "options.customization")?;
        if !undefined(c, function_name) || !undefined(c, customization) {
            let missing = c.undefined();
            parameters.extend_from_slice(&[missing, missing, function_name, customization]);
        }
    }
    let handle = c.construct_rooted(native, native, &parameters)?;
    let implicit_length = (!copying && undefined(c, length)).then_some(algorithm);
    initialize_instance(c, data, this, handle, options, implicit_length)
}
fn parameter(c: &mut Context<'_>, value: RootId, name: &str) -> Result<RootId, RootedError> {
    if undefined(c, value) {
        return Ok(value);
    }
    let bytes = buffer_source(c, value, name, None)?;
    if c.with_buffer_bytes_rooted(bytes, |bytes| bytes.contains(&0))
        .unwrap_or(false)
    {
        return Err(super::util_shared_vm::argument_value_error(
            c,
            value,
            name,
            "must not contain NUL bytes",
        )?);
    }
    Ok(bytes)
}
fn buffer_source(
    c: &mut Context<'_>,
    value: RootId,
    name: &str,
    encoding: Option<RootId>,
) -> Result<RootId, RootedError> {
    if c.value_type_rooted(value) == Some(ValueType::String) {
        let units = c.to_string_code_units(value)?;
        let encoding = match encoding {
            Some(encoding) if !nullish(c, encoding) => c.to_string(encoding)?,
            _ => "utf8".to_owned(),
        };
        let normalized = if encoding == "buffer" {
            Some("utf8")
        } else {
            super::buffer_codec::canonical_encoding(&encoding)
        };
        let normalized = normalized.ok_or_else(|| {
            match c.type_error_rooted(&format!("Unknown encoding: {encoding}")) {
                Ok(error) => match super::node_errors_shared_vm::decorate_type_error(
                    c,
                    error,
                    "ERR_UNKNOWN_ENCODING",
                ) {
                    Ok(()) => c.throw(error),
                    Err(error) => error,
                },
                Err(error) => error,
            }
        })?;
        return c.uint8_array_from_rooted(super::buffer_codec::encode_units(&units, normalized));
    }
    if c.has_builtin_brand_rooted(value, BuiltinBrand::AnyArrayBuffer)?
        || c.has_builtin_brand_rooted(value, BuiltinBrand::ArrayBufferView)?
    {
        return Ok(value);
    }
    Err(super::util_shared_vm::argument_type_error_with_expected(
        c,
        value,
        name,
        "of type string or an instance of ArrayBuffer, Buffer, TypedArray, or DataView",
    )?)
}
pub(crate) fn hmac(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let constructor = get(c, data, HMAC)?;
    if !c.instance_of_rooted(this, constructor)? {
        return create_hmac(c, this, args);
    }
    let algorithm = arg(c, args, 0);
    validate_string(c, algorithm, "hmac")?;
    let key = arg(c, args, 1);
    let options = arg(c, args, 2);
    let encoding = if c.truthy_rooted(options)? {
        get(c, options, "encoding")?
    } else {
        c.undefined()
    };
    if !nullish(c, encoding) {
        validate_string(c, encoding, "options.encoding")?;
    }
    let key = if c.value_type_rooted(key) == Some(ValueType::String)
        || c.has_builtin_brand_rooted(key, BuiltinBrand::AnyArrayBuffer)?
        || c.has_builtin_brand_rooted(key, BuiltinBrand::ArrayBufferView)?
    {
        buffer_source(c, key, "key", Some(encoding))?
    } else {
        let undefined = c.undefined();
        let material = super::crypto_key_shared_vm::secret_material(c, undefined, &[key])?;
        buffer_source(c, material, "key", Some(encoding))?
    };
    let native = get(c, data, NATIVE_HMAC)?;
    let handle = c.construct_rooted(native, native, &[])?;
    invoke(c, handle, "init", &[algorithm, key])?;
    initialize_instance(c, data, this, handle, options, None)?;
    Ok(c.undefined())
}
pub(crate) fn copy(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    ensure_active(c, data, this)?;
    let handle = native_handle(c, data, this)?;
    let options = arg(c, args, 0);
    let constructor = get(c, data, HASH)?;
    c.construct_rooted(constructor, constructor, &[handle, options])
}
pub(crate) fn update(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    ensure_active(c, data, this)?;
    let input = arg(c, args, 0);
    let encoding = arg(c, args, 1);
    if c.value_type_rooted(input) == Some(ValueType::String) {
        if c.string_text(encoding)?
            .and_then(|name| super::buffer_codec::canonical_encoding(&name))
            == Some("hex")
        {
            let units = c.to_string_code_units(input)?;
            if units.len() % 2 != 0 {
                // A normalized hex label is three ASCII letters: literal quoting is the
                // exact default inspection, without loading the general object inspector.
                let undefined = c.undefined();
                let inspected = super::util_shared_vm::inspect_quote(c, undefined, &[encoding])?;
                let name: Vec<_> = "encoding".encode_utf16().collect();
                let error = super::util_shared_vm::argument_value_error_inspected_units(
                    c,
                    &name,
                    &format!("is invalid for data of length {}", units.len()),
                    inspected,
                )?;
                if let Some(exception) = error.exception {
                    super::node_errors_shared_vm::hide_type_error(
                        c,
                        exception,
                        "ERR_INVALID_ARG_VALUE",
                    )?;
                }
                return Err(error);
            }
        }
    } else if !c.has_builtin_brand_rooted(input, BuiltinBrand::ArrayBufferView)? {
        return Err(super::util_shared_vm::argument_type_error_with_expected(
            c,
            input,
            "data",
            "of type string or an instance of Buffer, TypedArray, or DataView",
        )?);
    }
    let handle = native_handle(c, data, this)?;
    let updated = invoke(c, handle, "update", &[input, encoding])?;
    if !c.truthy_rooted(updated)? {
        return Err(crypto_error(c, HASH_UPDATE_FAILED, "Hash update failed")?);
    }
    Ok(this)
}
fn digest<const IS_HMAC: bool>(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
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
    let yes = c.boolean(true);
    c.assign_property_rooted(state, finalized, yes)?;
    Ok(result)
}
pub(crate) fn hash_digest(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    digest::<false>(c, this, args)
}
pub(crate) fn hmac_digest(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    digest::<true>(c, this, args)
}
pub(crate) fn transform(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let handle = native_handle(c, data, this)?;
    let chunk = arg(c, args, 0);
    let encoding = arg(c, args, 1);
    let callback = arg(c, args, 2);
    let result = invoke(c, handle, "update", &[chunk, encoding])?;
    let undefined = c.undefined();
    if c.truthy_rooted(result)? {
        c.call_rooted(callback, undefined, &[])?;
        return Ok(c.undefined());
    }
    let error = crypto_error(c, HASH_UPDATE_FAILED, "Hash update failed")?;
    c.call_rooted(
        callback,
        undefined,
        &[error.exception.expect("coded error has an exception")],
    )
}
fn flush<const IS_HMAC: bool>(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let handle = native_handle(c, data, this)?;
    let bytes = invoke(c, handle, "digest", &[])?;
    invoke(c, this, "push", &[bytes])?;
    if IS_HMAC {
        let (state, finalized) = state(c, data, this)?;
        let yes = c.boolean(true);
        c.assign_property_rooted(state, finalized, yes)?;
    }
    let callback = arg(c, args, 0);
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[])?;
    Ok(c.undefined())
}
pub(crate) fn hash_flush(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    flush::<false>(c, this, args)
}
pub(crate) fn hmac_flush(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    flush::<true>(c, this, args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quench_runtime::{Engine, ExecutionMode, Runtime, Value};

    #[test]
    fn public_hash_policy_matches_node_without_stream_demand() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/crypto_hash_public_oracle.js"
        );
        let oracle = std::process::Command::new("node")
            .args(["-e", "const fs=require('node:fs'),vm=require('node:vm'); console.log(vm.runInThisContext(fs.readFileSync(process.argv[1],'utf8'))(require('node:crypto')));", path])
            .env_remove("NODE_OPTIONS").output().unwrap();
        assert!(
            oracle.status.success(),
            "{}",
            String::from_utf8_lossy(&oracle.stderr)
        );
        let expected = String::from_utf8(oracle.stdout).unwrap();
        let source = std::fs::read_to_string(path).unwrap();
        let program =
            Engine::specialize(&format!("globalThis.exerciseHash = {source};"), path).unwrap();
        for mode in [ExecutionMode::Interpreted, ExecutionMode::Compiled] {
            let mut runtime = Runtime::new(NodeHost::new(vec!["quench-node".into()]));
            runtime.set_execution_mode(mode);
            runtime.execute(&program).unwrap();
            let global = runtime.global_root().unwrap();
            let exercise = runtime
                .get_named_property_rooted(global, "exerciseHash")
                .unwrap();
            // This unit isolates digest policy. Stream state is never requested; production
            // installation supplies stream.Transform, and its full integration remains separate.
            let parent = runtime.get_named_property_rooted(global, "Object").unwrap();
            let module = runtime.object_rooted().unwrap();
            let install = runtime
                .host_function(crate::host::shared_vm::operation("cryptoHashPublicInstall"))
                .unwrap();
            let undefined = runtime.root(Value::UNDEFINED);
            runtime
                .call_rooted(install, undefined, &[module, parent])
                .unwrap();
            let result = runtime.call_rooted(exercise, undefined, &[module]).unwrap();
            let actual = runtime.string_text_rooted(result).unwrap().unwrap();
            assert_eq!(
                actual.lines().count(),
                expected.lines().count(),
                "mode={mode:?}"
            );
            let mut differences = Vec::new();
            for (actual, expected) in actual.lines().zip(expected.lines()) {
                if actual != expected {
                    differences.push(format!("expected {expected}\nactual   {actual}"));
                }
            }
            assert!(
                differences.is_empty(),
                "mode={mode:?}\n{}",
                differences.join("\n")
            );
        }
    }
}
