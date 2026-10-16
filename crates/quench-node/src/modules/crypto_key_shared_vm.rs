//! Native key slot ownership for the configured Node key classes.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
// Node crypto_keys.cc's native key type and slot tuple ABI.
const KEY_TYPE_SECRET: f64 = 0.0;
const KEY_TYPE_PUBLIC: f64 = 1.0;
const KEY_TYPE_PRIVATE: f64 = 2.0;
const KEY_SLOT_COUNT: usize = 2;
const CRYPTO_KEY_SLOT_COUNT: usize = 7;
const CACHE: &str = "\0builtin:crypto/native-key-slots";
fn get(c: &mut Context<'_>, o: RootId, n: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(o, n)
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
        set(c, record, "bytes", bytes)?;
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
            ] {
                let method = function(c, state, name, false)?;
                set(c, proto, name, method)?;
            }
        }
    }
    for (name, n) in [
        ("kKeyTypeSecret", KEY_TYPE_SECRET),
        ("kKeyTypePublic", KEY_TYPE_PUBLIC),
        ("kKeyTypePrivate", KEY_TYPE_PRIVATE),
        ("kKeyFormatDER", 0.0),
        ("kKeyFormatPEM", 1.0),
        ("kKeyFormatJWK", 2.0),
        ("kKeyEncodingPKCS1", 0.0),
        ("kKeyEncodingPKCS8", 1.0),
        ("kKeyEncodingSPKI", 2.0),
        ("kKeyEncodingSEC1", 3.0),
    ] {
        let value = c.number(n);
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
            c.call_rooted(arg, missing, &[base])
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
            let bytes = args.get(1).copied().unwrap_or(missing);
            let Some(bytes) = c.view_bytes_rooted(bytes) else {
                return Err(fail(c, "Key material must be a byte view"));
            };
            // Secret key bytes have no ASN.1 policy; asymmetric parsing remains on the
            // existing OpenSSL key operations until that handle ABI is implemented.
            if kind != KEY_TYPE_SECRET {
                return Err(fail(
                    c,
                    "Asymmetric native key initialization is unsupported",
                ));
            }
            let material = c.uint8_array_rooted(&bytes)?;
            set(c, value, "bytes", material)?;
            set(c, value, "type", arg)?;
            Ok(c.boolean(true))
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
                Err(fail(c, "Invalid native key receiver"))
            }
        }
        "getKeyType" | "getSymmetricKeySize" | "export" | "equals" => {
            let value = lookup(c, state, "handles", receiver)?;
            if !c.is_object_rooted(value)? {
                return Err(fail(c, "Invalid key handle"));
            }
            if op == "getKeyType" {
                return get(c, value, "type");
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
