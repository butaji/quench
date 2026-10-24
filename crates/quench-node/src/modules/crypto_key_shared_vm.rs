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
// Do not use the VM's `\0quench:` internal-property prefix here: owned-clone
// intentionally omits those keys from ordinary own-key enumeration.
const CLONE_MARKER: &str = "__quench_crypto_keyobject_clone_v1__";
const CRYPTO_CLONE_MARKER: &str = "__quench_cryptokey_clone_v1__";
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

/// Same-realm structured cloning creates a fresh KeyObject wrapper while
/// retaining its immutable native KeyObjectHandle and source-owned brand.
pub(crate) fn clone_key_object(
    c: &mut Context<'_>,
    value: RootId,
) -> Result<Option<RootId>, RootedError> {
    let state = state(c)?;
    let slots = key_slots(c, state, value)?;
    if !c.is_object_rooted(slots)? {
        return Ok(None);
    }
    let kind = get(c, slots, "0")?;
    let kind = c.to_number(kind)? as u32;
    // createNativeKeyObjectClass's source callback returns the public base and
    // then its secret, public, and private subclasses in this stable order.
    let index = match kind {
        value if value == super::crypto_key_layout::Kind::Secret as u32 => 1,
        value if value == super::crypto_key_layout::Kind::Public as u32 => 2,
        value if value == super::crypto_key_layout::Kind::Private as u32 => 3,
        _ => return Ok(None),
    };
    let classes = get(c, state, "keyObjectClasses")?;
    if !c.is_object_rooted(classes)? {
        return Err(RootedError::host("KeyObject classes are not initialized"));
    }
    let index = c.number(index as f64);
    let constructor = c.get_property_rooted(classes, index)?;
    if !c.is_callable_rooted(constructor)? {
        return Err(RootedError::host(
            "KeyObject clone constructor is unavailable",
        ));
    }
    let handle = get(c, slots, "1")?;
    let clone = c.construct_rooted(constructor, constructor, &[handle])?;
    Ok(Some(clone))
}

/// Replace KeyObjects in a worker/message payload with clone-safe records.
/// The native handle is exported as DER/raw bytes, then re-imported in the
/// receiving runtime so its private WeakMap brands are established there.
#[cfg(quench_openssl_provider)]
pub(crate) fn prepare_key_objects_for_transport(
    c: &mut Context<'_>,
    value: RootId,
) -> Result<RootId, RootedError> {
    fn walk(
        c: &mut Context<'_>,
        value: RootId,
        seen: &mut Vec<(RootId, RootId, bool)>,
    ) -> Result<(RootId, bool), RootedError> {
        if !c.is_object_rooted(value)? {
            return Ok((value, false));
        }
        if let Some(record) = key_object_transport_record(c, value)? {
            return Ok((record, true));
        }
        if let Some(record) = crypto_key_transport_record(c, value)? {
            return Ok((record, true));
        }
        if c.clone_metadata_rooted(value)?.is_some() || c.builtin_data_rooted(value)?.is_some() {
            return Ok((value, false));
        }
        for (old, clone, contains_key) in seen.iter() {
            if c.strict_equal_rooted(*old, value)? {
                return Ok((*clone, *contains_key));
            }
        }
        let clone = if c.is_array_rooted(value)? {
            c.array_rooted(&[])?
        } else {
            c.object_rooted()?
        };
        let seen_index = seen.len();
        seen.push((value, clone, false));
        let mut contains_key = false;
        for key in c.enumerable_own_keys_rooted(value, false, true)? {
            let child = c.get_property_rooted(value, key)?;
            let (child, found) = walk(c, child, seen)?;
            contains_key |= found;
            c.define_data_property_rooted(clone, key, child, true, true, true)?;
        }
        seen[seen_index].2 = contains_key;
        Ok((clone, contains_key))
    }
    let mut seen = Vec::new();
    let (clone, contains_key) = walk(c, value, &mut seen)?;
    Ok(if contains_key { clone } else { value })
}

#[cfg(quench_openssl_provider)]
fn key_object_transport_record(
    c: &mut Context<'_>,
    value: RootId,
) -> Result<Option<RootId>, RootedError> {
    let state = state(c)?;
    let slots = key_slots(c, state, value)?;
    if !c.is_object_rooted(slots)? {
        return Ok(None);
    }
    let kind = get(c, slots, "0")?;
    let kind = c.to_number(kind)? as u32;
    let handle = get(c, slots, "1")?;
    let (name, bytes) = if kind == super::crypto_key_layout::Kind::Secret as u32 {
        let Some(bytes) = super::crypto_key_material_shared_vm::secret(c, handle)? else {
            return Err(RootedError::host(
                "Secret KeyObject material is unavailable",
            ));
        };
        ("secret", bytes.to_vec())
    } else {
        let Some(key) = super::crypto_asymmetric_shared_vm::key(c, handle)? else {
            return Err(RootedError::host(
                "Asymmetric KeyObject material is unavailable",
            ));
        };
        let (name, encoding) = if kind == super::crypto_key_layout::Kind::Public as u32 {
            ("public", super::crypto_asymmetric::Encoding::Spki)
        } else if kind == super::crypto_key_layout::Kind::Private as u32 {
            ("private", super::crypto_asymmetric::Encoding::Pkcs8)
        } else {
            return Ok(None);
        };
        let bytes = key
            .export(super::crypto_asymmetric::Format::Der, encoding, None, None)
            .map_err(|error| RootedError::host(format!("{error:?}")))?;
        (name, bytes)
    };
    let record = c.null_object_rooted()?;
    let marker = c.boolean(true);
    set(c, record, CLONE_MARKER, marker)?;
    let kind = c.string_rooted(name);
    set(c, record, "type", kind)?;
    let bytes = c.uint8_array_rooted(&bytes)?;
    set(c, record, "data", bytes)?;
    Ok(Some(record))
}

#[cfg(quench_openssl_provider)]
fn crypto_key_transport_record(
    c: &mut Context<'_>,
    value: RootId,
) -> Result<Option<RootId>, RootedError> {
    let state = state(c)?;
    let slots = lookup(c, state, "cryptoKeys", value)?;
    if !c.is_object_rooted(slots)? {
        return Ok(None);
    }
    let kind = get(c, slots, "0")?;
    let kind = c.to_number(kind)?;
    let handle = get(c, slots, "4")?;
    let (transport_type, bytes) = if kind == KEY_TYPE_SECRET {
        let Some(bytes) = super::crypto_key_material_shared_vm::secret(c, handle)? else {
            return Err(RootedError::host(
                "CryptoKey secret material is unavailable",
            ));
        };
        ("secret", bytes.to_vec())
    } else if kind == KEY_TYPE_PUBLIC || kind == KEY_TYPE_PRIVATE {
        let Some(key) = super::crypto_asymmetric_shared_vm::key(c, handle)? else {
            return Err(RootedError::host(
                "Asymmetric CryptoKey material is unavailable",
            ));
        };
        let (transport_type, encoding) = if kind == KEY_TYPE_PUBLIC {
            (
                "asymmetric-public",
                super::crypto_asymmetric::Encoding::Spki,
            )
        } else {
            (
                "asymmetric-private",
                super::crypto_asymmetric::Encoding::Pkcs8,
            )
        };
        let bytes = key
            .export(super::crypto_asymmetric::Format::Der, encoding, None, None)
            .map_err(|error| RootedError::host(format!("{error:?}")))?;
        (transport_type, bytes)
    } else {
        return Ok(None);
    };
    let record = c.null_object_rooted()?;
    let marker = c.boolean(true);
    set(c, record, CRYPTO_CLONE_MARKER, marker)?;
    let transport_type = c.string_rooted(transport_type);
    set(c, record, "type", transport_type)?;
    let algorithm = get(c, slots, "2")?;
    let usages = get(c, slots, "3")?;
    let extractable = get(c, slots, "1")?;
    set(c, record, "algorithm", algorithm)?;
    set(c, record, "usages", usages)?;
    set(c, record, "extractable", extractable)?;
    let bytes = c.uint8_array_rooted(&bytes)?;
    set(c, record, "data", bytes)?;
    Ok(Some(record))
}

/// Rehydrate transport records after a message graph enters this runtime.
#[cfg(quench_openssl_provider)]
pub(crate) fn revive_key_objects_after_transport(
    c: &mut Context<'_>,
    value: RootId,
) -> Result<RootId, RootedError> {
    fn walk(
        c: &mut Context<'_>,
        value: RootId,
        seen: &mut Vec<RootId>,
    ) -> Result<RootId, RootedError> {
        if !c.is_object_rooted(value)? {
            return Ok(value);
        }
        if let Some(key) = key_object_from_transport_record(c, value)? {
            return Ok(key);
        }
        if let Some(key) = crypto_key_from_transport_record(c, value)? {
            return Ok(key);
        }
        if c.clone_metadata_rooted(value)?.is_some() || c.builtin_data_rooted(value)?.is_some() {
            return Ok(value);
        }
        for old in seen.iter() {
            if c.strict_equal_rooted(*old, value)? {
                return Ok(value);
            }
        }
        seen.push(value);
        for key in c.enumerable_own_keys_rooted(value, false, true)? {
            let child = c.get_property_rooted(value, key)?;
            let revived = walk(c, child, seen)?;
            if !c.strict_equal_rooted(child, revived)? {
                c.define_data_property_rooted(value, key, revived, true, true, true)?;
            }
        }
        Ok(value)
    }
    walk(c, value, &mut Vec::new())
}

#[cfg(quench_openssl_provider)]
fn key_object_from_transport_record(
    c: &mut Context<'_>,
    record: RootId,
) -> Result<Option<RootId>, RootedError> {
    let marker = c.string_rooted(CLONE_MARKER);
    let descriptor = c.own_property_descriptor_rooted(record, marker)?;
    if !c.is_object_rooted(descriptor)? {
        return Ok(None);
    }
    let marked = get(c, descriptor, "value")?;
    if !c.truthy_rooted(marked)? {
        return Ok(None);
    }
    let kind_value = get(c, record, "type")?;
    let kind = c.string_text(kind_value)?;
    let data = get(c, record, "data")?;
    let bytes = c
        .bytes_prefix_rooted(data, usize::MAX)
        .ok_or_else(|| fail(c, "Invalid KeyObject transport record"))?;
    let (handle, index) = match kind.as_deref() {
        Some("secret") => (generated_secret(c, Material::new(bytes), None)?, 1u32),
        Some("public") | Some("private") => {
            let private = kind.as_deref() == Some("private");
            let owned = super::crypto_asymmetric::OwnedKey::import(
                if private {
                    super::crypto_asymmetric::KeyKind::Private
                } else {
                    super::crypto_asymmetric::KeyKind::Public
                },
                &bytes,
                super::crypto_asymmetric::Format::Der,
                Some(if private {
                    super::crypto_asymmetric::Encoding::Pkcs8
                } else {
                    super::crypto_asymmetric::Encoding::Spki
                }),
                None,
                None,
            )
            .map_err(|error| RootedError::host(format!("{error:?}")))?;
            (
                generated_asymmetric(c, Arc::new(owned), None)?,
                if private { 3 } else { 2 },
            )
        }
        _ => return Err(fail(c, "Invalid KeyObject transport type")),
    };
    let state = state(c)?;
    let mut classes = get(c, state, "keyObjectClasses")?;
    if !c.is_object_rooted(classes)? {
        // Incoming workerData/messages can be decoded before user code has
        // required node:crypto. Initialize the source class factory first so
        // revived objects carry the real private KeyObject brands.
        crate::host::shared_vm::commonjs::node_library_module(
            c,
            "crypto.js",
            "internal/crypto/keys.js",
        )?;
        classes = get(c, state, "keyObjectClasses")?;
    }
    let index = c.number(index as f64);
    let constructor = c.get_property_rooted(classes, index)?;
    if !c.is_callable_rooted(constructor)? {
        return Err(RootedError::host(
            "KeyObject clone constructor is unavailable",
        ));
    }
    Ok(Some(c.construct_rooted(
        constructor,
        constructor,
        &[handle],
    )?))
}

#[cfg(quench_openssl_provider)]
fn crypto_key_from_transport_record(
    c: &mut Context<'_>,
    record: RootId,
) -> Result<Option<RootId>, RootedError> {
    let marker = c.string_rooted(CRYPTO_CLONE_MARKER);
    let descriptor = c.own_property_descriptor_rooted(record, marker)?;
    if !c.is_object_rooted(descriptor)? {
        return Ok(None);
    }
    let marked = get(c, descriptor, "value")?;
    if !c.truthy_rooted(marked)? {
        return Ok(None);
    }
    let data = get(c, record, "data")?;
    let bytes = c
        .bytes_prefix_rooted(data, usize::MAX)
        .ok_or_else(|| fail(c, "Invalid CryptoKey transport record"))?;
    let algorithm = get(c, record, "algorithm")?;
    let usages = get(c, record, "usages")?;
    let extractable = get(c, record, "extractable")?;
    let transport_type = get(c, record, "type")?;
    let transport_type = c.string_text(transport_type)?;
    let state = state(c)?;
    let constructor = get(c, state, "internalCryptoKeyConstructor")?;
    if !c.is_callable_rooted(constructor)? {
        // A worker may receive workerData before user code loads crypto.
        crate::host::shared_vm::commonjs::node_library_module(
            c,
            "crypto.js",
            "internal/crypto/keys.js",
        )?;
    }
    let key = match transport_type.as_deref() {
        Some("asymmetric-public") | Some("asymmetric-private") => {
            let private = transport_type.as_deref() == Some("asymmetric-private");
            let owned = super::crypto_asymmetric::OwnedKey::import(
                if private {
                    super::crypto_asymmetric::KeyKind::Private
                } else {
                    super::crypto_asymmetric::KeyKind::Public
                },
                &bytes,
                super::crypto_asymmetric::Format::Der,
                Some(if private {
                    super::crypto_asymmetric::Encoding::Pkcs8
                } else {
                    super::crypto_asymmetric::Encoding::Spki
                }),
                None,
                None,
            )
            .map_err(|error| RootedError::host(format!("{error:?}")))?;
            generated_asymmetric(c, Arc::new(owned), Some((algorithm, usages, extractable)))?
        }
        Some("secret") | None => generated_secret(
            c,
            Material::new(bytes),
            Some((algorithm, usages, extractable)),
        )?,
        _ => return Err(fail(c, "Invalid CryptoKey transport type")),
    };
    Ok(Some(key))
}

/// Same-realm structured cloning creates a fresh CryptoKey wrapper while
/// retaining its immutable native handle and WebCrypto slot values.
pub(crate) fn clone_crypto_key(
    c: &mut Context<'_>,
    value: RootId,
) -> Result<Option<RootId>, RootedError> {
    let state = state(c)?;
    let slots = lookup(c, state, "cryptoKeys", value)?;
    if !c.is_object_rooted(slots)? {
        return Ok(None);
    }
    let slot = |c: &mut Context<'_>, index: usize| get(c, slots, &index.to_string());
    let handle = slot(c, 4)?;
    let algorithm = slot(c, 2)?;
    let usages_mask = slot(c, 3)?;
    let extractable = slot(c, 1)?;
    // Hybrid KEM keys keep their secondary KeyObjectHandle and seed in the
    // private CryptoKey slot cache. The host-side brand record predates those
    // slots for keys constructed by Node's internal KEM module, so read the
    // canonical values through Node's slot accessors before making a fresh
    // wrapper. This preserves hidden native material without aliasing the
    // CryptoKey object itself.
    let keys_module = crate::host::shared_vm::commonjs::node_library_module(
        c,
        "crypto.js",
        "internal/crypto/keys.js",
    )?;
    let undefined = c.undefined();
    let secondary_getter = get(c, keys_module, "getCryptoKeySecondaryHandle")?;
    let secondary_handle = c.call_rooted(secondary_getter, undefined, &[value])?;
    let seed_getter = get(c, keys_module, "getCryptoKeySeedData")?;
    let seed_data = c.call_rooted(seed_getter, undefined, &[value])?;
    let constructor = get(c, state, "internalCryptoKeyConstructor")?;
    if !c.is_callable_rooted(constructor)? {
        return Err(RootedError::host("SDK InternalCryptoKey not initialized"));
    }
    let clone = c.construct_rooted(
        constructor,
        constructor,
        &[
            handle,
            algorithm,
            usages_mask,
            extractable,
            secondary_handle,
            seed_data,
        ],
    )?;
    Ok(Some(clone))
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

/// Resolve the private native handle stored in a branded Node KeyObject.
/// Public fields and lookalike objects are ignored; the slots are retrieved
/// only from the private WeakMap installed by the crypto key classes.
#[cfg(quench_openssl_provider)]
pub(crate) fn key_object_handle(
    c: &mut Context<'_>,
    key: RootId,
) -> Result<Option<RootId>, RootedError> {
    let state = state(c)?;
    let slots = key_slots(c, state, key)?;
    if !c.is_object_rooted(slots)? {
        return Ok(None);
    }
    let kind = get(c, slots, "0")?;
    let kind = c.rooted_value(kind).and_then(|value| value.as_number());
    if kind != Some(KEY_TYPE_PUBLIC) && kind != Some(KEY_TYPE_PRIVATE) {
        return Ok(None);
    }
    Ok(Some(get(c, slots, "1")?))
}

/// Return the branded public KeyObject type without exposing its native
/// handle. Diffie-Hellman uses this to report Node's key-object type errors
/// before treating a SecretKeyObject as BufferSource input.
#[cfg(quench_openssl_provider)]
pub(crate) fn key_object_type(
    c: &mut Context<'_>,
    key: RootId,
) -> Result<Option<&'static str>, RootedError> {
    let state = state(c)?;
    let slots = key_slots(c, state, key)?;
    if !c.is_object_rooted(slots)? {
        return Ok(None);
    }
    let kind = get(c, slots, "0")?;
    let kind = c.rooted_value(kind).and_then(|value| value.as_number());
    Ok(match kind {
        Some(value) if value == KEY_TYPE_SECRET => Some("secret"),
        Some(value) if value == KEY_TYPE_PUBLIC => Some("public"),
        Some(value) if value == KEY_TYPE_PRIVATE => Some("private"),
        _ => None,
    })
}

/// Return the canonical native handle for a branded WebCrypto secret key.
/// The legacy `node:crypto` cipher surface still accepts CryptoKey values and
/// must resolve their material through the same backing as subtle operations.
pub(crate) fn secret_handle(
    c: &mut Context<'_>,
    key: RootId,
) -> Result<Option<RootId>, RootedError> {
    let state = state(c)?;
    let slots = lookup(c, state, "cryptoKeys", key)?;
    if !c.is_object_rooted(slots)? {
        return Ok(None);
    }
    let kind = get(c, slots, "0")?;
    if c.to_number(kind)? != KEY_TYPE_SECRET {
        return Ok(None);
    }
    Ok(Some(get(c, slots, "4")?))
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
                "rawSeed",
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
            if op == "keyClass" {
                set(c, state, "keyObjectClasses", classes)?;
            } else {
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
            let format = args.get(2).copied().unwrap_or(missing);
            let bytes = if c.to_number(format)? == 2.0 {
                // WebCrypto's AES/HMAC imports pass the already-validated JWK
                // to KeyObjectHandle.init(). Decode its octet key here, just
                // as Node's native KeyObjectHandle does, instead of treating
                // the JWK object itself as a BufferSource.
                let kty = get(c, bytes, "kty")?;
                if c.to_string(kty)? != "oct" {
                    return Err(fail(c, "Invalid JWK key type"));
                }
                let encoded = get(c, bytes, "k")?;
                let encoded = c.to_string(encoded)?;
                super::buffer_codec::base64_decode(encoded.as_bytes())
            } else {
                c.bytes_prefix_rooted(bytes, usize::MAX)
                    .ok_or_else(|| fail(c, "Key material must be a byte view"))?
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
                Err(invalid_this(
                    c,
                    if op == "keySlots" {
                        "KeyObject"
                    } else {
                        "CryptoKey"
                    },
                ))
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
        "rawSeed" => super::crypto_asymmetric_shared_vm::raw_seed(c, receiver),
        #[cfg(quench_openssl_provider)]
        "exportECPrivatePkcs8" => {
            super::crypto_asymmetric_shared_vm::export_ec_private_pkcs8(c, receiver)
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
    if let Some(handle) = secret_handle(c, value)? {
        if let Some(material) = super::crypto_key_material_shared_vm::secret(c, handle)? {
            return super::buffer_shared_vm::from_bytes(c, &material);
        }
    }
    Ok(value)
}
