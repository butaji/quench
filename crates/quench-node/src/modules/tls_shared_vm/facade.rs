//! Callable constructor presentation delegates TLS policy to Rust.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
const FACTORY: &str = quench_js_check::checked_js!(
    r#"(register, initialize, create) => {
 'use strict';
 class NativeSecureContext { constructor() { register(this); } }
 Object.defineProperty(NativeSecureContext.prototype, '_external', {
   configurable: true,
   get() { throw new TypeError('Illegal invocation'); }
 });
 function SecureContext(secureProtocol, secureOptions, minVersion, maxVersion) {
   return initialize(this, this instanceof SecureContext, secureProtocol, secureOptions, minVersion, maxVersion);
 }
 return { SecureContext, NativeSecureContext,
   createSecureContext: function createSecureContext(options) { return create(options); } };
}"#
);
fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}
fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    context.set_property_rooted(object, key, value, object)?;
    Ok(())
}
pub(crate) fn install(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let shared = context.host_mut().shared_state();
    if let Some(api) = shared.borrow().tls_contexts.api {
        return Ok(api);
    }
    let register =
        context.host_function(crate::host::shared_vm::operation("tlsContextRegister"))?;
    let initialize =
        context.host_function(crate::host::shared_vm::operation("tlsContextOuterInit"))?;
    let create = context.host_function(crate::host::shared_vm::operation("tlsContextCreate"))?;
    let factory = context.evaluate_script_rooted(FACTORY, "node:tls/context-facades.js")?;
    let undefined = context.undefined();
    let api = context.call_rooted(factory, undefined, &[register, initialize, create])?;
    let native = get(context, api, "NativeSecureContext")?;
    let name = context.string_rooted("name");
    let text = context.string_rooted("SecureContext");
    context.define_data_property_rooted(native, name, text, false, false, true)?;
    let binding = context.object_rooted()?;
    #[cfg(quench_openssl_provider)]
    super::super::crypto_provider_shared_vm::install(context, binding)?;
    let key = context.string_rooted("SecureContext");
    context.define_data_property_rooted(binding, key, native, true, true, true)?;
    if super::super::crypto_engine_shared_vm::SUPPORTED {
        let function =
            context.host_function(crate::host::shared_vm::operation("cryptoSetEngineBinding"))?;
        let key = context.string_rooted("name");
        let name = context.string_rooted("setEngine");
        context.define_data_property_rooted(function, key, name, false, false, true)?;
        set(context, binding, "setEngine", function)?;
    }
    super::super::crypto_secret_job_shared_vm::install(context, binding)?;
    #[cfg(quench_openssl_provider)]
    super::super::crypto_hash_shared_vm::install(context, binding)?;
    #[cfg(quench_openssl_provider)]
    super::super::crypto_signature_shared_vm::install(context, binding)?;
    #[cfg(quench_openssl_provider)]
    super::super::crypto_rsa_shared_vm::install(context, binding)?;
    set(context, api, "binding", binding)?;
    let prototype = get(context, native, "prototype")?;
    for (name, operation) in [
        ("init", "tlsContextInit"),
        ("setOptions", "tlsContextSetOptions"),
        ("getMinProto", "tlsContextGetMinProto"),
        ("getMaxProto", "tlsContextGetMaxProto"),
    ] {
        let function = context.host_function(crate::host::shared_vm::operation(operation))?;
        let key = context.string_rooted("name");
        let text = context.string_rooted(name);
        context.define_data_property_rooted(function, key, text, false, false, true)?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(prototype, key, function, true, true, true)?;
    }
    for name in super::configuration::METHODS {
        if matches!(*name, "setClientCertEngine" | "setEngineKey")
            && !super::super::crypto_engine_shared_vm::SUPPORTED
        {
            continue;
        }
        let action = context.string_rooted(name);
        let function = context.host_function_with_data(
            crate::host::shared_vm::operation("tlsContextConfigure"),
            action,
        )?;
        let key = context.string_rooted("name");
        let text = context.string_rooted(name);
        context.define_data_property_rooted(function, key, text, false, false, true)?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(prototype, key, function, true, true, true)?;
    }
    let api = context.retain(api)?;
    shared.borrow_mut().tls_contexts.api = Some(api);
    Ok(api)
}
fn absent(context: &NativeContext<'_, NodeHost>, value: RootId) -> bool {
    context
        .rooted_value(value)
        .is_some_and(|value| value.is_null() || value.is_undefined())
}
fn protocol_json(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<String, RootedError> {
    let value = context.stringify_json_rooted(value)?;
    if context
        .rooted_value(value)
        .is_some_and(|value| value.is_undefined())
    {
        Ok("undefined".into())
    } else {
        context.to_string(value)
    }
}
fn version(
    context: &mut NativeContext<'_, NodeHost>,
    api: RootId,
    value: RootId,
    minimum: bool,
) -> Result<RootId, RootedError> {
    let value = if absent(context, value) {
        let module = get(context, api, "module")?;
        get(
            context,
            module,
            if minimum {
                "DEFAULT_MIN_VERSION"
            } else {
                "DEFAULT_MAX_VERSION"
            },
        )?
    } else {
        value
    };
    let raw = match context.string_text(value)?.as_deref() {
        Some("TLSv1") => openssl_sys::TLS1_VERSION,
        Some("TLSv1.1") => openssl_sys::TLS1_1_VERSION,
        Some("TLSv1.2") => openssl_sys::TLS1_2_VERSION,
        Some("TLSv1.3") => openssl_sys::TLS1_3_VERSION,
        _ => {
            let detail = protocol_json(context, value)?;
            let which = if minimum { "minimum" } else { "maximum" };
            let error = context.type_error_rooted(&format!(
                "{detail} is not a valid {which} TLS protocol version"
            ))?;
            super::super::node_errors_shared_vm::decorate_type_error(
                context,
                error,
                "ERR_TLS_INVALID_PROTOCOL_VERSION",
            )?;
            return Err(context.throw(error));
        }
    };
    Ok(context.number(f64::from(raw)))
}
pub(crate) fn initialize(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let receiver = args.first().copied().unwrap_or_else(|| context.undefined());
    let existing = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let values = (2..6)
        .map(|index| {
            args.get(index)
                .copied()
                .unwrap_or_else(|| context.undefined())
        })
        .collect::<Vec<_>>();
    let api = install(context)?;
    if !context.truthy_rooted(existing)? {
        let constructor = get(context, api, "SecureContext")?;
        return context.construct_rooted(constructor, constructor, &values);
    }
    let [protocol, options, minimum, maximum] = values.as_slice() else {
        unreachable!()
    };
    if context.truthy_rooted(*protocol)? {
        for value in [*minimum, *maximum] {
            if !absent(context, value) {
                let version = protocol_json(context, value)?;
                let protocol = protocol_json(context, *protocol)?;
                let error = context.type_error_rooted(&format!(
                    "TLS protocol version {version} conflicts with secureProtocol {protocol}"
                ))?;
                super::super::node_errors_shared_vm::decorate_type_error(
                    context,
                    error,
                    "ERR_TLS_PROTOCOL_VERSION_CONFLICT",
                )?;
                return Err(context.throw(error));
            }
        }
    }
    let minimum = version(context, api, *minimum, true)?;
    let maximum = version(context, api, *maximum, false)?;
    let constructor = get(context, api, "contextConstructor")?;
    let inner = context.construct_rooted(constructor, constructor, &[])?;
    set(context, receiver, "context", inner)?;
    let inner = get(context, receiver, "context")?;
    let init = get(context, inner, "init")?;
    context.call_rooted(init, inner, &[*protocol, minimum, maximum])?;
    if context.truthy_rooted(*options)? {
        validate_options(context, *options)?;
        let inner = get(context, receiver, "context")?;
        let setter = get(context, inner, "setOptions")?;
        context.call_rooted(setter, inner, &[*options])?;
    }
    Ok(context.undefined())
}
fn validate_options(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<(), RootedError> {
    const MAX_SAFE_INTEGER: f64 = ((1_u64 << f64::MANTISSA_DIGITS) - 1) as f64;
    let Some(number) = context
        .rooted_value(value)
        .and_then(|value| value.as_number())
    else {
        return Err(super::super::util_shared_vm::argument_type_error(
            context,
            value,
            "secureOptions",
            "number",
        )?);
    };
    if !number.is_finite() || number.fract() != 0.0 || number.abs() > MAX_SAFE_INTEGER {
        let received = super::super::util_shared_vm::range_error_received(context, value)?;
        let rule = if !number.is_finite() || number.fract() != 0.0 {
            "an integer".to_owned()
        } else {
            format!(">= -{MAX_SAFE_INTEGER} && <= {MAX_SAFE_INTEGER}")
        };
        let error = context.range_error_rooted(&format!(
            "The value of \"secureOptions\" is out of range. It must be {rule}. Received {received}"
        ))?;
        super::super::node_errors_shared_vm::decorate_range_error(
            context,
            error,
            "ERR_OUT_OF_RANGE",
        )?;
        super::super::node_errors_shared_vm::hide_range_error(context, error, "ERR_OUT_OF_RANGE")?;
        return Err(context.throw(error));
    }
    Ok(())
}
pub(crate) fn create(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let mut options = args.first().copied().unwrap_or_else(|| context.undefined());
    if !context.truthy_rooted(options)? {
        options = context.null_object_rooted()?;
    }
    let honor = get(context, options, "honorCipherOrder")?;
    let minimum = get(context, options, "minVersion")?;
    let maximum = get(context, options, "maxVersion")?;
    let protocol = get(context, options, "secureProtocol")?;
    let mut bits = get(context, options, "secureOptions")?;
    if context.truthy_rooted(honor)? {
        let preference = context.number(openssl_sys::SSL_OP_CIPHER_SERVER_PREFERENCE as f64);
        bits = context.bitwise_or_rooted(bits, preference)?;
    }
    let api = install(context)?;
    let constructor = get(context, api, "SecureContext")?;
    let outer = context.construct_rooted(
        constructor,
        constructor,
        &[protocol, bits, minimum, maximum],
    )?;
    super::configuration::configure(context, outer, options)?;
    Ok(outer)
}

pub(crate) fn binding(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let api = install(context)?;
    get(context, api, "binding")
}
