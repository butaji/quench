//! Native key slot ownership for the configured Node key classes.
use super::crypto_secret_material::Material;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::sync::Arc;
type Context<'a> = NativeContext<'a, NodeHost>;
// Node crypto_keys.cc's native key type and slot tuple ABI.
const KEY_TYPE_SECRET: f64 = super::crypto_key_layout::Kind::Secret as u32 as f64;
const KEY_TYPE_PUBLIC: f64 = super::crypto_key_layout::Kind::Public as u32 as f64;
const KEY_TYPE_PRIVATE: f64 = super::crypto_key_layout::Kind::Private as u32 as f64;
const KEY_SLOT_COUNT: usize = 2;
const CRYPTO_KEY_SLOT_COUNT: usize = 7;
const CACHE: &str = "\0builtin:crypto/native-key-slots";
fn get(c: &mut Context<'_>, o: RootId, n: &str) -> Result<RootId, RootedError> {
    let k = c.string_rooted(n);
    c.get_property_rooted(o, k)
}
fn set(c: &mut Context<'_>, o: RootId, n: &str, v: RootId) -> Result<(), RootedError> {
    let k = c.string_rooted(n);
    c.define_data_property_rooted(o, k, v, true, true, true)?;
    Ok(())
}
fn fail(c: &mut Context<'_>, message: &str) -> RootedError {
    match c.type_error_rooted(message) {
        Ok(e) => c.throw(e),
        Err(e) => e,
    }
}
// NativeKeyObject::GetSlots and NativeCryptoKey::GetSlots reject unbranded
// receivers before the source classes access their private lazy caches.
fn invalid_this(c: &mut Context<'_>, kind: &str) -> RootedError {
    let result = (|| {
        let error = c.type_error_rooted(&format!("Value of \"this\" must be of type {kind}"))?;
        let code = c.string_rooted("ERR_INVALID_THIS");
        set(c, error, "code", code)?;
        Ok::<_, RootedError>(c.throw(error))
    })();
    result.unwrap_or_else(|error| error)
}
fn function(
    c: &mut Context<'_>,
    state: RootId,
    op: &str,
    constructor: bool,
) -> Result<RootId, RootedError> {
    let display = match op {
        "handle" | "keyBase" | "cryptoBase" => "",
        "keyClass" => "createNativeKeyObjectClass",
        "cryptoClass" => "createCryptoKeyClass",
        "isKey" => "isKeyObject",
        "keySlots" => "getKeyObjectSlots",
        "cryptoSlots" => "getCryptoKeySlots",
        _ => op,
    };
    let operation = c.string_rooted(op);
    let data = c.array_rooted(&[state, operation])?;
    let function = if constructor {
        c.host_constructor_with_data(crate::host::shared_vm::operation("cryptoKeySlots"), data)?
    } else {
        c.host_function_with_data(crate::host::shared_vm::operation("cryptoKeySlots"), data)?
    };
    let key = c.string_rooted("name");
    let display = c.string_rooted(display);
    c.define_data_property_rooted(function, key, display, false, false, true)?;
    if matches!(op, "getKeyObjectHandle" | "getKeyObjectType") {
        let key = c.string_rooted("length");
        let length = c.number(1.0);
        c.define_data_property_rooted(function, key, length, false, false, true)?;
    }
    Ok(function)
}

fn state(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    if let Some(value) = c
        .host_mut()
        .shared_state()
        .borrow()
        .builtin_cache
        .get(CACHE)
        .copied()
    {
        return Ok(value);
    }
    let state = c.null_object_rooted()?;
    let global = c.global_root()?;
    let wm = match c.startup_data_property_rooted(global, "WeakMap") {
        Some(value) => value,
        None => get(c, global, "WeakMap")?,
    };
    for name in ["handles", "keys", "cryptoKeys"] {
        let map = c.construct_rooted(wm, wm, &[])?;
        set(c, state, name, map)?;
    }
    let map = get(c, state, "handles")?;
    let prototype = c.prototype_rooted(map)?;
    let setter = match c.startup_data_property_rooted(prototype, "set") {
        Some(value) => value,
        None => get(c, map, "set")?,
    };
    set(c, state, "set", setter)?;
    let root = c.retain(state)?;
    c.host_mut()
        .shared_state()
        .borrow_mut()
        .builtin_cache
        .insert(CACHE.into(), root);
    Ok(state)
}
fn store(
    c: &mut Context<'_>,
    state: RootId,
    map: &str,
    key: RootId,
    value: RootId,
) -> Result<(), RootedError> {
    let map = get(c, state, map)?;
    let setter = get(c, state, "set")?;
    c.call_rooted(setter, map, &[key, value])?;
    Ok(())
}
fn lookup(
    c: &mut Context<'_>,
    state: RootId,
    map: &str,
    key: RootId,
) -> Result<RootId, RootedError> {
    let map = get(c, state, map)?;
    c.weak_map_get_rooted(map, key)
}
/// Retain the canonical provider's existing private key maps, never public fields.
pub(crate) fn capture_brands(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let state = state(c)?;
    for (i, name) in ["secretBrands", "asymmetricBrands"].into_iter().enumerate() {
        if let Some(value) = args.get(i) {
            set(c, state, name, *value)?;
        }
    }
    Ok(c.undefined())
}
fn key_slots(c: &mut Context<'_>, state: RootId, key: RootId) -> Result<RootId, RootedError> {
    let existing = lookup(c, state, "keys", key)?;
    if c.is_object_rooted(existing)? {
        return Ok(existing);
    }
    for (name, secret) in [("secretBrands", true), ("asymmetricBrands", false)] {
        let map = get(c, state, name)?;
        if !c.is_object_rooted(map)? {
            continue;
        }
        let data = c.weak_map_get_rooted(map, key)?;
        if !c.is_object_rooted(data)? {
            continue;
        }
        let (kind, bytes) = if secret {
            (KEY_TYPE_SECRET, data)
        } else {
            let kind = get(c, data, "type")?;
            let kind = c.to_string(kind)?;
            (
                if kind == "public" {
                    KEY_TYPE_PUBLIC
                } else {
                    KEY_TYPE_PRIVATE
                },
                get(c, data, "der")?,
            )
        };
        let constructor = c
            .host_mut()
            .shared_state()
            .borrow()
            .builtin_cache
            .get("\0builtin:buffer/FastBuffer")
            .copied()
            .ok_or_else(|| RootedError::host("FastBuffer is not initialized"))?;
        let bytes = c.construct_rooted(constructor, constructor, &[bytes])?;
        let handle_ctor = get(c, state, "handleConstructor")?;
        let handle_ctor = if c.is_callable_rooted(handle_ctor)? {
            handle_ctor
        } else {
            binding(c)?;
            get(c, state, "handleConstructor")?
        };
        let handle = c.construct_rooted(handle_ctor, handle_ctor, &[])?;
        let record = lookup(c, state, "handles", handle)?;
        let kind = c.number(kind);
        set(c, record, "type", kind)?;
        if secret {
            let bytes = c
                .bytes_prefix_rooted(bytes, usize::MAX)
                .ok_or_else(|| fail(c, "Legacy secret bytes are unavailable"))?;
            super::crypto_key_material_shared_vm::register_secret(
                c,
                handle,
                Arc::new(Material::new(bytes)),
            )?;
        } else {
            set(c, record, "bytes", bytes)?;
        }

        let slots = c.array_rooted(&[kind, handle])?;
        store(c, state, "keys", key, slots)?;
        return Ok(slots);
    }
    Ok(c.undefined())
}
/// Source algorithms and the existing key provider share private slot access.
pub(crate) fn install_provider_accessors(
    c: &mut Context<'_>,
    module: RootId,
) -> Result<(), RootedError> {
    let state = state(c)?;
    for (name, op) in [
        ("getKeyObjectHandle", "getKeyObjectHandle"),
        ("getKeyObjectType", "getKeyObjectType"),
        ("isKeyObject", "isKey"),
        ("isCryptoKey", "isCryptoKey"),
    ] {
        let f = function(c, state, op, false)?;
        let key = c.string_rooted("length");
        let length = c.number(1.0);
        c.define_data_property_rooted(f, key, length, false, false, true)?;
        set(c, module, name, f)?;
    }
    Ok(())
}
pub(crate) fn binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let state = state(c)?;
    let cached = get(c, state, "binding")?;
    if c.is_object_rooted(cached)? {
        return Ok(cached);
    }
    let module = c.object_rooted()?;
    for (name, op, ctor) in [
        ("KeyObjectHandle", "handle", true),
        ("createNativeKeyObjectClass", "keyClass", false),
        ("createCryptoKeyClass", "cryptoClass", false),
        ("isKeyObject", "isKey", false),
        ("isCryptoKey", "isCryptoKey", false),
        ("getKeyObjectSlots", "keySlots", false),
        ("getCryptoKeySlots", "cryptoSlots", false),
    ] {
        let f = function(c, state, op, ctor)?;
        set(c, module, name, f)?;
        if name == "KeyObjectHandle" {
            set(c, state, "handleConstructor", f)?;
            let proto = get(c, f, "prototype")?;
            for name in [
                "init",
                "export",
                "equals",
                "getKeyType",
                "getSymmetricKeySize",
                "getAsymmetricKeyType",
                "keyDetail",
                "exportJwk",
                "rawPublicKey",
                "rawPrivateKey",
                "exportECPublicRaw",
                "exportECPrivateRaw",
                "exportECPrivatePkcs8",
                "checkEcKeyData",
            ] {
                let method = function(c, state, name, false)?;
                set(c, proto, name, method)?;
            }
        }
    }
    for &(name, n) in super::crypto_key_layout::CONSTANTS {
        let value = c.number(f64::from(n));
        set(c, module, name, value)?;
    }
    set(c, state, "binding", module)?;
    Ok(module)
}
pub(crate) fn operation(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let state = get(c, data, "0")?;
    let op = get(c, data, "1")?;
    let op = c.to_string(op)?;
    let missing = c.undefined();
    let arg = args.first().copied().unwrap_or(missing);
    match op.as_str() {
        "keyClass" | "cryptoClass" => {
            let base = function(
                c,
                state,
                if op == "keyClass" {
                    "keyBase"
                } else {
                    "cryptoBase"
                },
                true,
            )?;
            let classes = c.call_rooted(arg, missing, &[base])?;
            if op == "cryptoClass" {
                let internal = get(c, classes, "1")?;
                set(c, state, "internalCryptoKeyConstructor", internal)?;
            }
            Ok(classes)
        }
        "handle" => {
            let value = c.null_object_rooted()?;
            store(c, state, "handles", receiver, value)?;
            Ok(receiver)
        }
        "init" => {
            let value = lookup(c, state, "handles", receiver)?;
            if !c.is_object_rooted(value)? {
                return Err(fail(c, "Invalid key handle"));
            }
            let kind = c.to_number(arg)?;
            #[cfg(quench_openssl_provider)]
            if kind != KEY_TYPE_SECRET {
                let kind = super::crypto_asymmetric_shared_vm::initialize(c, receiver, args)?;
                let kind = c.number(f64::from(kind));
                set(c, value, "type", kind)?;
                let format = args.get(2).copied().unwrap_or(missing);
                return if c.to_number(format)? == 2.0 {
                    Ok(kind)
                } else {
                    Ok(c.undefined())
                };
            }
            #[cfg(not(quench_openssl_provider))]
            if kind != KEY_TYPE_SECRET {
                let bytes = args.get(1).copied().unwrap_or(missing);
                let bytes = c
                    .bytes_prefix_rooted(bytes, usize::MAX)
                    .ok_or_else(|| fail(c, "Key material must be a byte view"))?;
                let bytes = c.uint8_array_rooted(&bytes)?;
                set(c, value, "bytes", bytes)?;
                set(c, value, "type", arg)?;
                return Ok(c.boolean(true));
            }
            let bytes = args.get(1).copied().unwrap_or(missing);
            let Some(bytes) = c.bytes_prefix_rooted(bytes, usize::MAX) else {
                return Err(fail(c, "Key material must be a byte view"));
            };
            super::crypto_key_material_shared_vm::register_secret(
                c,
                receiver,
                Arc::new(Material::new(bytes)),
            )?;
            set(c, value, "type", arg)?;
            Ok(c.undefined())
        }

        "keyBase" | "cryptoBase" => {
            let handle = lookup(c, state, "handles", arg)?;
            if !c.is_object_rooted(handle)? {
                return Err(fail(c, "Invalid key handle"));
            }
            let kind = get(c, handle, "type")?;
            if c.rooted_value(kind).is_some_and(|v| v.is_undefined()) {
                return Err(fail(c, "Uninitialized key handle"));
            }
            let slots = if op == "keyBase" {
                c.array_rooted(&[kind, arg])?
            } else {
                let a = |i: usize| args.get(i).copied().unwrap_or(missing);
                c.array_rooted(&[kind, a(3), a(1), a(2), arg, a(4), a(5)])?
            };
            store(
                c,
                state,
                if op == "keyBase" {
                    "keys"
                } else {
                    "cryptoKeys"
                },
                receiver,
                slots,
            )?;
            // Source-created secrets and the existing cipher/KDF provider share
            // this exact native byte owner. No second material copy or key ID.
            if op == "keyBase"
                && c.rooted_value(kind).and_then(|value| value.as_number()) == Some(KEY_TYPE_SECRET)
            {
                let brands = get(c, state, "secretBrands")?;
                if c.is_object_rooted(brands)? {
                    let bytes = arg;
                    let setter = get(c, state, "set")?;
                    c.call_rooted(setter, brands, &[receiver, bytes])?;
                }
            }
            Ok(receiver)
        }
        "getKeyObjectHandle" | "getKeyObjectType" => {
            let slots = key_slots(c, state, arg)?;
            if !c.is_object_rooted(slots)? {
                return Err(fail(c, "Invalid key receiver"));
            }
            if op == "getKeyObjectHandle" {
                get(c, slots, "1")
            } else {
                let kind = get(c, slots, "0")?;
                let kind = c.to_number(kind)?;
                Ok(c.string_rooted(if kind == KEY_TYPE_SECRET {
                    "secret"
                } else if kind == KEY_TYPE_PUBLIC {
                    "public"
                } else {
                    "private"
                }))
            }
        }
        "isKey" => {
            for name in ["keys", "secretBrands", "asymmetricBrands"] {
                let map = get(c, state, name)?;
                if !c.is_object_rooted(map)? {
                    continue;
                }
                let value = c.weak_map_get_rooted(map, arg)?;
                if c.is_object_rooted(value)? {
                    return Ok(c.boolean(true));
                }
            }
            Ok(c.boolean(false))
        }
        "isCryptoKey" | "keySlots" | "cryptoSlots" => {
            let map = if matches!(op.as_str(), "isKey" | "keySlots") {
                "keys"
            } else {
                "cryptoKeys"
            };
            let slots = if map == "keys" {
                key_slots(c, state, arg)?
            } else {
                lookup(c, state, map, arg)?
            };
            if op.starts_with("is") {
                Ok(c.boolean(c.rooted_value(slots).is_some_and(|v| !v.is_undefined())))
            } else if c.is_object_rooted(slots)? {
                // Source owns its lazy cache tuple; it must not mutate native slots.
                let count = if op == "keySlots" {
                    KEY_SLOT_COUNT
                } else {
                    CRYPTO_KEY_SLOT_COUNT
                };
                let mut values = Vec::with_capacity(count);
                for index in 0..count {
                    values.push(get(c, slots, &index.to_string())?);
                }
                c.array_rooted(&values)
            } else {
                Err(invalid_this(c, if op == "keySlots" { "KeyObject" } else { "CryptoKey" }))
            }
        }
        #[cfg(quench_openssl_provider)]
        "getAsymmetricKeyType" => super::crypto_asymmetric_shared_vm::key_type(c, receiver),
        #[cfg(quench_openssl_provider)]
        "keyDetail" => super::crypto_asymmetric_shared_vm::key_detail(c, receiver, arg),
        #[cfg(quench_openssl_provider)]
        "exportJwk" => {
            #[cfg(quench_openssl_provider)]
            let allow = {
                let allow = args.get(1).copied().unwrap_or(missing);
                c.truthy_rooted(allow)?
            };
            #[cfg(quench_openssl_provider)]
            if super::crypto_asymmetric_shared_vm::key(c, receiver)?.is_some() {
                return super::crypto_asymmetric_shared_vm::export_jwk(c, receiver, arg, allow);
            }
            {
                let bytes = secret_bytes(c, receiver)?;
                let kty = c.string_rooted("oct");
                set(c, arg, "kty", kty)?;
                let mut text = super::buffer_codec::base64_encode(&bytes, false, true);
                let encoded = c.string_rooted(&text);
                super::crypto_secret_material::cleanse(unsafe { text.as_bytes_mut() });
                set(c, arg, "k", encoded)?;
                Ok(arg)
            }
        }
        #[cfg(quench_openssl_provider)]
        "rawPublicKey" | "exportECPublicRaw" => {
            let compressed = op == "exportECPublicRaw" && c.to_number(arg)? == 2.0;
            super::crypto_asymmetric_shared_vm::raw(c, receiver, false, compressed)
        }
        #[cfg(quench_openssl_provider)]
        "rawPrivateKey" | "exportECPrivateRaw" => {
            super::crypto_asymmetric_shared_vm::raw(c, receiver, true, false)
        }
        #[cfg(quench_openssl_provider)]
        "exportECPrivatePkcs8" => {
            let format = c.number(0.0);
            let encoding = c.number(1.0);
            super::crypto_asymmetric_shared_vm::export(c, receiver, &[format, encoding])
        }
        #[cfg(quench_openssl_provider)]
        "checkEcKeyData" => {
            let key = super::crypto_asymmetric_shared_vm::key(c, receiver)?;
            Ok(c.boolean(key.is_some_and(|key| key.check_ec_key_data())))
        }
        "getKeyType" | "getSymmetricKeySize" | "export" | "equals" => {
            let value = lookup(c, state, "handles", receiver)?;
            if !c.is_object_rooted(value)? {
                return Err(fail(c, "Invalid key handle"));
            }
            if op == "getKeyType" {
                return get(c, value, "type");
            }
            #[cfg(quench_openssl_provider)]
            if let Some(key) = super::crypto_asymmetric_shared_vm::key(c, receiver)? {
                return match op.as_str() {
                    "export" => super::crypto_asymmetric_shared_vm::export(c, receiver, args),
                    "equals" => {
                        let other = super::crypto_asymmetric_shared_vm::key(c, arg)?;
                        Ok(c.boolean(
                            other.is_some_and(|other| {
                                key.kind() == other.kind() && key.equals(&other)
                            }),
                        ))
                    }
                    _ => Err(fail(c, "Asymmetric key has no symmetric size")),
                };
            }
            if let Some(bytes) = super::crypto_key_material_shared_vm::secret(c, receiver)? {
                return match op.as_str() {
                    "getSymmetricKeySize" => Ok(c.number(bytes.len() as f64)),
                    "export" => super::buffer_shared_vm::from_bytes(c, &bytes),
                    "equals" => {
                        let other = super::crypto_key_material_shared_vm::secret(c, arg)?;
                        Ok(c.boolean(other.is_some_and(|other| {
                            bytes.len() == other.len() && openssl::memcmp::eq(&bytes, &other)
                        })))
                    }
                    _ => Err(fail(c, "Invalid secret key operation")),
                };
            }
            let bytes = get(c, value, "bytes")?;
            let Some(bytes) = c.view_bytes_rooted(bytes) else {
                return Err(fail(c, "Uninitialized key handle"));
            };
            match op.as_str() {
                "getSymmetricKeySize" => Ok(c.number(bytes.len() as f64)),
                "export" => {
                    let bytes = c.uint8_array_rooted(&bytes)?;
                    let constructor = c
                        .host_mut()
                        .shared_state()
                        .borrow()
                        .builtin_cache
                        .get("\0builtin:buffer/FastBuffer")
                        .copied()
                        .ok_or_else(|| RootedError::host("FastBuffer is not initialized"))?;
                    c.construct_rooted(constructor, constructor, &[bytes])
                }
                _ => {
                    let other = lookup(c, state, "handles", arg)?;
                    if !c.is_object_rooted(other)? {
                        return Err(fail(c, "Invalid key handle"));
                    }
                    let kind = get(c, value, "type")?;
                    let kind = c.to_number(kind)?;
                    let other_kind = get(c, other, "type")?;
                    let other_kind = c.to_number(other_kind)?;
                    if kind != other_kind {
                        return Ok(c.boolean(false));
                    }
                    let other = get(c, other, "bytes")?;
                    let Some(other) = c.view_bytes_rooted(other) else {
                        return Err(fail(c, "Uninitialized key handle"));
                    };
                    let equal = if kind == KEY_TYPE_SECRET {
                        bytes.len() == other.len() && openssl::memcmp::eq(&bytes, &other)
                    } else {
                        match (public_material(&bytes), public_material(&other)) {
                            (Ok(left), Ok(right)) => left.public_eq(&right),
                            (Err(error), _) | (_, Err(error)) => {
                                let error =
                                    super::crypto_error_shared_vm::create(c, &error, false)?;
                                return Err(c.throw(error));
                            }
                        }
                    };
                    Ok(c.boolean(equal))
                }
            }
        }
        _ => Err(RootedError::host("Unknown native key operation")),
    }
}

fn public_material(
    bytes: &[u8],
) -> Result<openssl::pkey::PKey<openssl::pkey::Public>, openssl::error::ErrorStack> {
    use openssl::pkey::PKey;
    PKey::public_key_from_der(bytes)
        .or_else(|_| PKey::public_key_from_pem(bytes))
        .or_else(|_| {
            let private =
                PKey::private_key_from_der(bytes).or_else(|_| PKey::private_key_from_pem(bytes))?;
            PKey::public_key_from_der(&private.public_key_to_der()?)
        })
}

/// Build actual generated-key handles using the same secret ownership/slot path.
/// No new brand map, global key IDs, or duplicated key material representation.
pub(crate) fn generated_secret(
    c: &mut Context<'_>,
    bytes: Material,
    crypto: Option<(RootId, RootId, RootId)>,
) -> Result<RootId, RootedError> {
    let binding = binding(c)?;
    let constructor = get(c, binding, "KeyObjectHandle")?;
    let handle = c.construct_rooted(constructor, constructor, &[])?;
    let state = state(c)?;
    let record = lookup(c, state, "handles", handle)?;
    super::crypto_key_material_shared_vm::register_secret(c, handle, Arc::new(bytes))?;
    let kind = c.number(KEY_TYPE_SECRET);
    set(c, record, "type", kind)?;
    if let Some((algorithm, usages, extractable)) = crypto {
        let constructor = get(c, state, "internalCryptoKeyConstructor")?;
        if !c.is_callable_rooted(constructor)? {
            return Err(RootedError::host("SDK InternalCryptoKey not initialized"));
        }
        c.construct_rooted(
            constructor,
            constructor,
            &[handle, algorithm, usages, extractable],
        )
    } else {
        Ok(handle)
    }
}

/// A native Hmac/HKDF retains the same immutable native secret owner. Guest
/// fields and mutable public constructors cannot grant key access.
#[cfg(quench_openssl_provider)]
pub(crate) fn secret_bytes(
    c: &mut Context<'_>,
    value: RootId,
) -> Result<Arc<Material>, RootedError> {
    if let Some(secret) = super::crypto_key_material_shared_vm::secret(c, value)? {
        return Ok(secret);
    }
    if let Some(bytes) = c.bytes_prefix_rooted(value, usize::MAX) {
        return Ok(Arc::new(Material::new(bytes)));
    }
    Err(fail(c, "Invalid native secret key handle"))
}
/// Actual asymmetric handles use the same source-owned KeyObject slot authority.
#[cfg(quench_openssl_provider)]
pub(crate) fn generated_asymmetric(
    c: &mut Context<'_>,
    key: std::sync::Arc<super::crypto_asymmetric::OwnedKey>,
    crypto: Option<(RootId, RootId, RootId)>,
) -> Result<RootId, RootedError> {
    let binding = binding(c)?;
    let constructor = get(c, binding, "KeyObjectHandle")?;
    let handle = c.construct_rooted(constructor, constructor, &[])?;
    let state = state(c)?;
    let record = lookup(c, state, "handles", handle)?;
    let kind = c.number(
        if key.kind() == super::crypto_asymmetric::KeyKind::Private {
            KEY_TYPE_PRIVATE
        } else {
            KEY_TYPE_PUBLIC
        },
    );
    set(c, record, "type", kind)?;
    super::crypto_asymmetric_shared_vm::register(c, handle, key)?;
    if let Some((algorithm, usages, extractable)) = crypto {
        let constructor = get(c, state, "internalCryptoKeyConstructor")?;
        if !c.is_callable_rooted(constructor)? {
            return Err(RootedError::host("SDK InternalCryptoKey not initialized"));
        }
        c.construct_rooted(
            constructor,
            constructor,
            &[handle, algorithm, usages, extractable],
        )
    } else {
        Ok(handle)
    }
}

/// Existing byte-oriented cipher/KDF adapters snapshot canonical native material
/// only when an operation consumes it. Old normalized arrays remain owned by
/// the legacy backend; neither path installs a second canonical secret backing.
pub(crate) fn secret_material(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| c.undefined());
    if let Some(material) = super::crypto_key_material_shared_vm::secret(c, value)? {
        return super::buffer_shared_vm::from_bytes(c, &material);
    }
    Ok(value)
}
