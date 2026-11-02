//! Structured cloning of KeyObjects and CryptoKeys.
//!
//! In one realm a clone shares the original's native key. Across runtimes (worker
//! messages) each key travels as a plain record of its encoded material, which the
//! receiving runtime imports again.

use super::keys::{self, Key};
use super::web;
use super::{get, put, Context};
use crate::modules::crypto_asymmetric::{Encoding, Format, KeyKind, OwnedKey};
use crate::modules::crypto_secret_material::Material;
use quench_runtime::{RootId, RootedError};
use std::sync::Arc;

/// The member that marks a transport record, and what it carries.
const RECORD_MARKER: &str = "__quench_crypto_key_v2__";
const KEY_OBJECT: &str = "KeyObject";
const CRYPTO_KEY: &str = "CryptoKey";

/// A same-realm clone of a KeyObject or CryptoKey; `None` for any other value.
pub(crate) fn clone_key(c: &mut Context<'_>, value: RootId) -> Result<Option<RootId>, RootedError> {
    if let Some(key) = keys::key_object_key(c, value)? {
        return keys::key_object(c, key).map(Some);
    }
    web::clone_crypto_key(c, value)
}

/// Whether `value` is a KeyObject or CryptoKey.
pub(crate) fn is_key(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    Ok(keys::key_of(c, value)?.is_some())
}

fn encoded(key: &Key) -> Result<(&'static str, Vec<u8>), RootedError> {
    match key {
        Key::Secret(bytes) => Ok(("secret", bytes.to_vec())),
        Key::Asymmetric(owned) => {
            let (name, encoding) = if owned.kind() == KeyKind::Public { ("public", Encoding::Spki) } else { ("private", Encoding::Pkcs8) };
            let bytes = owned.export(Format::Der, encoding, None, None).map_err(|error| RootedError::host(format!("{error:?}")))?;
            Ok((name, bytes))
        }
    }
}

fn decoded(type_name: &str, bytes: Vec<u8>) -> Result<Key, RootedError> {
    let (kind, encoding) = match type_name {
        "secret" => return Ok(Key::Secret(Material::new(bytes))),
        "public" => (KeyKind::Public, Encoding::Spki),
        _ => (KeyKind::Private, Encoding::Pkcs8),
    };
    let owned = OwnedKey::import(kind, &bytes, Format::Der, Some(encoding), None, None).map_err(|error| RootedError::host(format!("{error:?}")))?;
    Ok(Key::Asymmetric(owned))
}

/// The transport record of a KeyObject or CryptoKey.
fn record(c: &mut Context<'_>, value: RootId) -> Result<Option<RootId>, RootedError> {
    let crypto_key = web::crypto_key_record(c, value)?;
    let Some(key) = keys::key_of(c, value)? else {
        return Ok(None);
    };
    let (type_name, bytes) = encoded(&key)?;
    let record = match crypto_key {
        Some(record) => {
            let kind = c.string_rooted(CRYPTO_KEY);
            put(c, record, RECORD_MARKER, kind)?;
            record
        }
        None => {
            let record = c.null_object_rooted()?;
            let kind = c.string_rooted(KEY_OBJECT);
            put(c, record, RECORD_MARKER, kind)?;
            record
        }
    };
    let type_value = c.string_rooted(type_name);
    put(c, record, "type", type_value)?;
    let data = c.uint8_array_rooted(&bytes)?;
    put(c, record, "data", data)?;
    Ok(Some(record))
}

/// The key a transport record describes.
fn revive(c: &mut Context<'_>, value: RootId) -> Result<Option<RootId>, RootedError> {
    let marker = c.string_rooted(RECORD_MARKER);
    let Some(fields) = c.own_property_fields_rooted(value, marker)? else {
        return Ok(None);
    };
    let kind = match fields.value {
        Some(kind) => c.string_text(kind)?,
        None => None,
    };
    let type_value = get(c, value, "type")?;
    let type_name = c.string_text(type_value)?.unwrap_or_default();
    let data = get(c, value, "data")?;
    let bytes = super::buffer_bytes(c, data).unwrap_or_default();
    let key = Arc::new(decoded(&type_name, bytes)?);
    match kind.as_deref() {
        Some(KEY_OBJECT) => keys::key_object(c, key).map(Some),
        Some(CRYPTO_KEY) => web::crypto_key_from_record(c, key, value).map(Some),
        _ => Ok(None),
    }
}

/// Whether `value` is a container a payload walk descends into.
fn is_container(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    Ok(c.is_object_rooted(value)? && c.clone_metadata_rooted(value)?.is_none() && c.builtin_data_rooted(value)?.is_none())
}

/// A payload with its keys replaced by transport records (the payload itself when it
/// holds none).
pub(crate) fn prepare(c: &mut Context<'_>, value: RootId) -> Result<RootId, RootedError> {
    fn walk(c: &mut Context<'_>, value: RootId, seen: &mut Vec<(RootId, RootId, bool)>) -> Result<(RootId, bool), RootedError> {
        if !c.is_object_rooted(value)? {
            return Ok((value, false));
        }
        if let Some(record) = record(c, value)? {
            return Ok((record, true));
        }
        if !is_container(c, value)? {
            return Ok((value, false));
        }
        for (old, copy, found) in seen.iter() {
            if c.strict_equal_rooted(*old, value)? {
                return Ok((*copy, *found));
            }
        }
        let copy = if c.is_array_rooted(value)? { c.array_rooted(&[])? } else { c.object_rooted()? };
        let index = seen.len();
        seen.push((value, copy, false));
        let mut found = false;
        for key in c.enumerable_own_keys_rooted(value, false, true)? {
            let child = c.get_property_rooted(value, key)?;
            let (child, child_found) = walk(c, child, seen)?;
            found |= child_found;
            c.define_data_property_rooted(copy, key, child, true, true, true)?;
        }
        seen[index].2 = found;
        Ok((copy, found))
    }
    let (copy, found) = walk(c, value, &mut Vec::new())?;
    Ok(if found { copy } else { value })
}

/// A received payload with its transport records imported as keys again.
pub(crate) fn restore(c: &mut Context<'_>, value: RootId) -> Result<RootId, RootedError> {
    fn walk(c: &mut Context<'_>, value: RootId, seen: &mut Vec<RootId>) -> Result<RootId, RootedError> {
        if !c.is_object_rooted(value)? {
            return Ok(value);
        }
        if let Some(key) = revive(c, value)? {
            return Ok(key);
        }
        if !is_container(c, value)? {
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

/// Whether a payload holds a key below its root.
pub(crate) fn contains_nested_key(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    fn walk(c: &mut Context<'_>, value: RootId, seen: &mut Vec<RootId>) -> Result<bool, RootedError> {
        if !is_container(c, value)? {
            return Ok(false);
        }
        for old in seen.iter() {
            if c.strict_equal_rooted(*old, value)? {
                return Ok(false);
            }
        }
        seen.push(value);
        for key in c.enumerable_own_keys_rooted(value, false, true)? {
            let child = c.get_property_rooted(value, key)?;
            if is_key(c, child)? || walk(c, child, seen)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
    if is_key(c, value)? {
        return Ok(false);
    }
    walk(c, value, &mut Vec::new())
}
