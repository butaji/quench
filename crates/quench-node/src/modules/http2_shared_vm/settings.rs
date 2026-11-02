//! Settings as JavaScript sees them (lib/internal/http2/util.js): the object form, its
//! validation, the SETTINGS entries it submits, and the packed `HTTP2-Settings` form.

use super::connection::Settings;
use super::faults::Fault;
use super::frame::{self, SettingId};
use super::MAX_INT;
use crate::modules::stream_shared_vm::{get, is_undefined, put, put_bool, put_number, Context, Outcome};
use quench_runtime::{RootId, RootedError};

/// Node's `MAX_ADDITIONAL_SETTINGS`.
const MAX_ADDITIONAL_SETTINGS: usize = 10;
/// The largest settings identifier.
const MAX_SETTING_ID: f64 = u16::MAX as f64;

/// How a setting appears in the object form.
#[derive(Clone, Copy)]
enum Shape {
    Number { minimum: f64, maximum: f64 },
    Boolean,
}

/// The object form's numeric and boolean settings, by property name and identifier.
const FIELDS: [(&str, SettingId, Shape); 7] = [
    ("headerTableSize", SettingId::HeaderTableSize, Shape::Number { minimum: 0.0, maximum: MAX_INT }),
    ("initialWindowSize", SettingId::InitialWindowSize, Shape::Number { minimum: 0.0, maximum: frame::MAX_WINDOW as f64 }),
    (
        "maxFrameSize",
        SettingId::MaxFrameSize,
        Shape::Number { minimum: frame::MIN_MAX_FRAME_SIZE as f64, maximum: frame::MAX_MAX_FRAME_SIZE as f64 },
    ),
    ("maxConcurrentStreams", SettingId::MaxConcurrentStreams, Shape::Number { minimum: 0.0, maximum: MAX_INT }),
    ("maxHeaderListSize", SettingId::MaxHeaderListSize, Shape::Number { minimum: 0.0, maximum: MAX_INT }),
    ("enablePush", SettingId::EnablePush, Shape::Boolean),
    ("enableConnectProtocol", SettingId::EnableConnectProtocol, Shape::Boolean),
];
/// `maxHeaderSize`, an alias of `maxHeaderListSize`.
const MAX_HEADER_SIZE: &str = "maxHeaderSize";

fn invalid_setting(c: &mut Context<'_>, name: &str, value: RootId, range: Option<(f64, f64)>) -> RootedError {
    let shown = match c.to_string(value) {
        Ok(shown) => shown,
        Err(error) => return error,
    };
    let fault = if range.is_some() { Fault::InvalidSettingRange } else { Fault::InvalidSettingValue };
    let error = match fault.error(c, &[name, &shown]) {
        Ok(error) => error,
        Err(error) => return error,
    };
    let fields = (|| {
        put(c, error, "actual", value)?;
        if let Some((minimum, maximum)) = range {
            put_number(c, error, "min", minimum)?;
            put_number(c, error, "max", maximum)?;
        }
        Ok::<_, RootedError>(())
    })();
    match fields {
        Ok(()) => c.throw(error),
        Err(failure) => failure,
    }
}

fn assert_within_range(c: &mut Context<'_>, name: &str, value: RootId, minimum: f64, maximum: f64) -> Result<(), RootedError> {
    if is_undefined(c, value) {
        return Ok(());
    }
    match c.rooted_value(value).and_then(|value| value.as_number()) {
        Some(number) if number >= minimum && number <= maximum => Ok(()),
        _ => Err(invalid_setting(c, name, value, Some((minimum, maximum)))),
    }
}

/// `validateSettings(settings)`.
pub(super) fn validate(c: &mut Context<'_>, settings: RootId) -> Result<(), RootedError> {
    if is_undefined(c, settings) {
        return Ok(());
    }
    let custom = get(c, settings, "customSettings")?;
    super::assert_is_object(c, custom, "customSettings", "Number")?;
    if c.truthy_rooted(custom)? {
        let keys = c.enumerable_own_keys_rooted(custom, false, true)?;
        if keys.len() > MAX_ADDITIONAL_SETTINGS {
            return Err(super::throw(c, Fault::TooManyCustomSettings, &[]));
        }
        for key in keys {
            let value = c.get_property_rooted(custom, key)?;
            let id = c.to_number(key)?;
            let id = c.number(id);
            assert_within_range(c, "customSettings:id", id, 0.0, MAX_SETTING_ID)?;
            let number = c.to_number(value)?;
            let number = c.number(number);
            assert_within_range(c, "customSettings:value", number, 0.0, MAX_INT)?;
        }
    }
    for (name, _, shape) in FIELDS {
        let value = get(c, settings, name)?;
        match shape {
            Shape::Number { minimum, maximum } => assert_within_range(c, name, value, minimum, maximum)?,
            Shape::Boolean => {
                if !is_undefined(c, value) && c.rooted_value(value).and_then(|value| value.as_bool()).is_none() {
                    return Err(invalid_setting(c, name, value, None));
                }
            }
        }
        if name == "maxHeaderListSize" {
            let alias = get(c, settings, MAX_HEADER_SIZE)?;
            assert_within_range(c, MAX_HEADER_SIZE, alias, minimum_of(shape), maximum_of(shape))?;
        }
    }
    Ok(())
}

fn minimum_of(shape: Shape) -> f64 {
    match shape {
        Shape::Number { minimum, .. } => minimum,
        Shape::Boolean => 0.0,
    }
}

fn maximum_of(shape: Shape) -> f64 {
    match shape {
        Shape::Number { maximum, .. } => maximum,
        Shape::Boolean => 1.0,
    }
}

/// `updateSettingsBuffer(settings)`: the entries a SETTINGS frame of `settings` lists, in
/// identifier order, then its custom settings.
pub(super) fn entries(c: &mut Context<'_>, settings: RootId) -> Result<Vec<(u16, u32)>, RootedError> {
    let mut known: Vec<(SettingId, u32)> = Vec::new();
    let mut custom: Vec<(u16, u32)> = Vec::new();
    let custom_settings = get(c, settings, "customSettings")?;
    if c.is_object_rooted(custom_settings)? {
        for key in c.enumerable_own_keys_rooted(custom_settings, false, true)? {
            let value = c.get_property_rooted(custom_settings, key)?;
            let Some(value) = c.rooted_value(value).and_then(|value| value.as_number()) else { continue };
            let id = c.to_number(key)?;
            if id.is_nan() || id <= 0.0 || id > MAX_SETTING_ID {
                let id = c.number(id);
                return Err(invalid_setting(c, "Range Error", id, Some((0.0, MAX_SETTING_ID))));
            }
            if value.is_nan() || value <= 0.0 || value > MAX_INT {
                let value = c.number(value);
                return Err(invalid_setting(c, "Range Error", value, Some((0.0, MAX_INT))));
            }
            match SettingId::ALL.into_iter().find(|known_id| f64::from(*known_id as u16) == id) {
                Some(known_id) => known.push((known_id, value as u32)),
                None => match custom.iter().position(|(existing, _)| f64::from(*existing) == id) {
                    Some(index) => custom[index].1 = value as u32,
                    None if custom.len() == MAX_ADDITIONAL_SETTINGS => {
                        return Err(super::throw(c, Fault::TooManyCustomSettings, &[]));
                    }
                    None => custom.push((id as u16, value as u32)),
                },
            }
        }
    }
    for (name, id, shape) in FIELDS {
        let value = get(c, settings, name)?;
        let value = if id == SettingId::MaxHeaderListSize {
            let alias = get(c, settings, MAX_HEADER_SIZE)?;
            if !is_undefined(c, alias) && !c.strict_equal_rooted(alias, value)? {
                super::emit_warning(c, "settings.maxHeaderSize overwrite settings.maxHeaderListSize", None)?;
                alias
            } else if c.rooted_value(value).and_then(|value| value.as_number()).is_some() {
                value
            } else {
                alias
            }
        } else {
            value
        };
        let entry = match shape {
            Shape::Number { .. } => c.rooted_value(value).and_then(|value| value.as_number()).map(|value| value as u32),
            Shape::Boolean => c.rooted_value(value).and_then(|value| value.as_bool()).map(u32::from),
        };
        if let Some(entry) = entry {
            known.retain(|(existing, _)| *existing != id);
            known.push((id, entry));
        }
    }
    known.sort_by_key(|(id, _)| *id as u16);
    let mut list: Vec<(u16, u32)> = known.into_iter().map(|(id, value)| (id as u16, value)).collect();
    list.extend(custom);
    Ok(list)
}

/// `getSettings(session, remote)`'s object.
pub(super) fn object(c: &mut Context<'_>, settings: &Settings) -> Outcome {
    let object = c.object_rooted()?;
    put_number(c, object, "headerTableSize", f64::from(settings.header_table_size))?;
    put_bool(c, object, "enablePush", settings.enable_push)?;
    put_number(c, object, "initialWindowSize", f64::from(settings.initial_window_size))?;
    put_number(c, object, "maxFrameSize", f64::from(settings.max_frame_size))?;
    put_number(c, object, "maxConcurrentStreams", f64::from(settings.max_concurrent_streams))?;
    put_number(c, object, "maxHeaderListSize", f64::from(settings.max_header_list_size))?;
    put_number(c, object, MAX_HEADER_SIZE, f64::from(settings.max_header_list_size))?;
    put_bool(c, object, "enableConnectProtocol", settings.enable_connect_protocol)?;
    Ok(object)
}

/// `http2.getDefaultSettings()`: Node's defaults, on a null-prototype object.
pub(super) fn default_settings(c: &mut Context<'_>) -> Outcome {
    let defaults = Settings { max_header_list_size: super::DEFAULT_MAX_HEADER_LIST_SIZE, ..Settings::INITIAL };
    let holder = c.null_object_rooted()?;
    put_number(c, holder, "headerTableSize", f64::from(defaults.header_table_size))?;
    put_bool(c, holder, "enablePush", defaults.enable_push)?;
    put_number(c, holder, "initialWindowSize", f64::from(defaults.initial_window_size))?;
    put_number(c, holder, "maxFrameSize", f64::from(defaults.max_frame_size))?;
    put_number(c, holder, "maxConcurrentStreams", f64::from(defaults.max_concurrent_streams))?;
    put_number(c, holder, MAX_HEADER_SIZE, f64::from(defaults.max_header_list_size))?;
    put_number(c, holder, "maxHeaderListSize", f64::from(defaults.max_header_list_size))?;
    put_bool(c, holder, "enableConnectProtocol", defaults.enable_connect_protocol)?;
    Ok(holder)
}

/// `http2.getPackedSettings(settings)`: the SETTINGS payload of the standard settings.
/// Like Node's binding, it yields `undefined` when custom settings are present.
pub(super) fn packed(c: &mut Context<'_>, settings: RootId) -> Outcome {
    super::assert_is_object(c, settings, "settings", "Object")?;
    validate(c, settings)?;
    let settings = if is_undefined(c, settings) { c.object_rooted()? } else { settings };
    let entries = entries(c, settings)?;
    if entries.iter().any(|(id, _)| !SettingId::ALL.iter().any(|known| *known as u16 == *id)) {
        return Ok(c.undefined());
    }
    let mut bytes = Vec::with_capacity(entries.len() * frame::SETTING_LENGTH);
    for (id, value) in entries {
        bytes.extend_from_slice(&id.to_be_bytes());
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    super::buffer(c, bytes)
}

/// `http2.getUnpackedSettings(buf, options)`.
pub(super) fn unpacked(c: &mut Context<'_>, buffer: RootId, options: RootId) -> Outcome {
    let Some(bytes) = c.view_bytes_rooted(buffer) else {
        return Err(crate::modules::util_shared_vm::argument_type_error_with_expected(
            c,
            buffer,
            "buf",
            &super::types_expectation("Buffer,TypedArray"),
        )?);
    };
    if bytes.len() % frame::SETTING_LENGTH != 0 {
        return Err(super::throw(c, Fault::InvalidPackedSettingsLength, &[]));
    }
    let settings = c.object_rooted()?;
    let mut custom: Option<RootId> = None;
    for entry in bytes.chunks_exact(frame::SETTING_LENGTH) {
        let id = frame::read_u16(entry);
        let value = f64::from(frame::read_u32(&entry[2..]));
        match SettingId::ALL.into_iter().find(|known| *known as u16 == id) {
            Some(SettingId::EnablePush) => put_bool(c, settings, "enablePush", value != 0.0)?,
            Some(SettingId::EnableConnectProtocol) => put_bool(c, settings, "enableConnectProtocol", value != 0.0)?,
            Some(SettingId::MaxHeaderListSize) => {
                put_number(c, settings, MAX_HEADER_SIZE, value)?;
                put_number(c, settings, "maxHeaderListSize", value)?;
            }
            Some(known) => {
                let name = FIELDS.iter().find(|(_, field, _)| *field == known).map_or("", |(name, _, _)| name);
                put_number(c, settings, name, value)?;
            }
            None => {
                let holder = match custom {
                    Some(holder) => holder,
                    None => {
                        let holder = c.object_rooted()?;
                        put(c, settings, "customSettings", holder)?;
                        custom = Some(holder);
                        holder
                    }
                };
                put_number(c, holder, &id.to_string(), value)?;
            }
        }
    }
    if c.is_object_rooted(options)? && crate::modules::stream_shared_vm::truthy_member(c, options, "validate")? {
        validate(c, settings)?;
    }
    Ok(settings)
}
