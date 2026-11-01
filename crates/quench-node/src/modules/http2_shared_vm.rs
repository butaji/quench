//! JavaScript host binding bridge for the native HTTP/2 session engine.
//!
//! Public HTTP/2 server and client adapters use these narrow operations to
//! pump protocol bytes over Quench's existing net/TLS sockets.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = c.string_rooted(name);
    c.get_property_rooted(object, key)
}

fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}

fn put_text(
    c: &mut Context<'_>,
    object: RootId,
    name: &str,
    value: &str,
) -> Result<(), RootedError> {
    let value = c.string_rooted(value);
    put(c, object, name, value)
}

fn put_number(
    c: &mut Context<'_>,
    object: RootId,
    name: &str,
    value: f64,
) -> Result<(), RootedError> {
    let value = c.number(value);
    put(c, object, name, value)
}

fn put_bool(
    c: &mut Context<'_>,
    object: RootId,
    name: &str,
    value: bool,
) -> Result<(), RootedError> {
    let value = c.boolean(value);
    put(c, object, name, value)
}

pub(crate) fn install_binding(c: &mut Context<'_>, binding: RootId) -> Result<(), RootedError> {
    for (name, operation) in [
        ("newSession", "http2NewSession"),
        ("setLocalWindowSize", "http2SetLocalWindowSize"),
        ("submitRstStream", "http2SubmitRstStream"),
        ("feed", "http2Feed"),
        ("drainOutbound", "http2DrainOutbound"),
        ("submitHeaders", "http2SubmitHeaders"),
        ("submitSettings", "http2SubmitSettings"),
        ("submitData", "http2SubmitData"),
        ("submitRequest", "http2SubmitRequest"),
        ("submitPushPromise", "http2SubmitPushPromise"),
        ("closeSession", "http2CloseSession"),
    ] {
        let function = c.host_function(crate::host::shared_vm::operation(operation))?;
        put(c, binding, name, function)?;
    }
    Ok(())
}

pub(crate) fn set_local_window_size(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = number_arg(c, args, 0)?;
    let size = number_arg(c, args, 1)?;
    let size = match i32::try_from(size) {
        Ok(size) => size,
        Err(_) => {
            let error = c.range_error_rooted("Invalid HTTP/2 connection window size")?;
            return Err(c.throw(error));
        }
    };
    crate::modules::http2_native::set_local_window_size(id, size)
        .map_err(|message| h2_error(c, message))?;
    Ok(c.undefined())
}

pub(crate) fn submit_rst_stream(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = number_arg(c, args, 0)?;
    let stream_id = number_arg(c, args, 1)?;
    let error_code = number_arg(c, args, 2)?;
    let stream_id = match i32::try_from(stream_id) {
        Ok(value) => value,
        Err(_) => {
            let error = c.range_error_rooted("Invalid HTTP/2 stream identifier")?;
            return Err(c.throw(error));
        }
    };
    let error_code = match u32::try_from(error_code) {
        Ok(value) => value,
        Err(_) => {
            let error = c.range_error_rooted("Invalid HTTP/2 stream error code")?;
            return Err(c.throw(error));
        }
    };
    crate::modules::http2_native::submit_rst_stream(id, stream_id, error_code)
        .map_err(|message| h2_error(c, message))?;
    Ok(c.undefined())
}

fn number_arg(c: &mut Context<'_>, args: &[RootId], index: usize) -> Result<u64, RootedError> {
    let value = args.get(index).copied().unwrap_or_else(|| c.undefined());
    let number = c.to_number(value)?;
    if !number.is_finite() || number < 0.0 || number.fract() != 0.0 || number > u64::MAX as f64 {
        let error = c.range_error_rooted("Invalid HTTP/2 native identifier")?;
        return Err(c.throw(error));
    }
    Ok(number as u64)
}

fn array_items(c: &mut Context<'_>, array: RootId) -> Result<Vec<RootId>, RootedError> {
    let length = get(c, array, "length")?;
    let length = c.to_number(length)?;
    if !length.is_finite() || length < 0.0 || length.fract() != 0.0 || length > 65_536.0 {
        let error = c.range_error_rooted("Invalid HTTP/2 header list")?;
        return Err(c.throw(error));
    }
    (0..length as usize)
        .map(|index| {
            let key = c.number(index as f64);
            c.get_property_rooted(array, key)
        })
        .collect()
}

fn header_pairs(
    c: &mut Context<'_>,
    value: RootId,
) -> Result<Vec<(Vec<u8>, Vec<u8>)>, RootedError> {
    let mut result = Vec::new();
    for pair in array_items(c, value)? {
        let fields = array_items(c, pair)?;
        if fields.len() < 2 {
            let error = c.type_error_rooted("HTTP/2 header entries must be pairs")?;
            return Err(c.throw(error));
        }
        let name = c.to_string(fields[0])?.into_bytes();
        let value = c.to_string(fields[1])?.into_bytes();
        result.push((name, value));
    }
    Ok(result)
}

fn settings_pairs(
    c: &mut Context<'_>,
    value: RootId,
) -> Result<Vec<(u16, u32)>, RootedError> {
    let mut result = Vec::new();
    for pair in array_items(c, value)? {
        let fields = array_items(c, pair)?;
        if fields.len() < 2 {
            let error = c.type_error_rooted("HTTP/2 settings entries must be pairs")?;
            return Err(c.throw(error));
        }
        let id = c.to_number(fields[0])?;
        let setting = c.to_number(fields[1])?;
        if !id.is_finite() || id < 0.0 || id > u16::MAX as f64 || id.fract() != 0.0 ||
            !setting.is_finite() || setting < 0.0 || setting > u32::MAX as f64 || setting.fract() != 0.0 {
            let error = c.range_error_rooted("Invalid HTTP/2 setting entry")?;
            return Err(c.throw(error));
        }
        result.push((id as u16, setting as u32));
    }
    Ok(result)
}

fn h2_error(c: &mut Context<'_>, message: String) -> RootedError {
    let error = c
        .error_rooted(&message)
        .expect("creating an Error object cannot fail");
    c.throw(error)
}

pub(crate) fn new_session(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let server = args
        .first()
        .is_some_and(|value| c.to_number(*value).unwrap_or(0.0) != 0.0);
    match crate::modules::http2_native::new_session(server) {
        Ok(id) => Ok(c.number(id as f64)),
        Err(error) => Err(h2_error(c, error)),
    }
}

pub(crate) fn feed(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = number_arg(c, args, 0)?;
    let Some(bytes) = args.get(1).and_then(|value| c.view_bytes_rooted(*value)) else {
        let error = c.type_error_rooted("HTTP/2 input must be a byte buffer")?;
        return Err(c.throw(error));
    };
    let events =
        crate::modules::http2_native::feed(id, &bytes).map_err(|error| h2_error(c, error))?;
    let mut values = Vec::with_capacity(events.len());
    for event in events {
        let object = c.object_rooted()?;
        match event {
            crate::modules::http2_native::H2Event::Headers {
                stream_id,
                headers,
                end_stream,
            } => {
                put_text(c, object, "type", "headers")?;
                put_number(c, object, "streamId", stream_id as f64)?;
                put_bool(c, object, "endStream", end_stream)?;
                let mut pairs = Vec::with_capacity(headers.len());
                for (name, value) in headers {
                    let name = c.string_rooted(&String::from_utf8_lossy(&name));
                    let value = c.string_rooted(&String::from_utf8_lossy(&value));
                    let pair = c.array_rooted(&[name, value])?;
                    pairs.push(pair);
                }
                let pairs = c.array_rooted(&pairs)?;
                put(c, object, "headers", pairs)?;
            }
            crate::modules::http2_native::H2Event::PushPromise {
                stream_id,
                parent_stream_id,
                headers,
            } => {
                put_text(c, object, "type", "push")?;
                put_number(c, object, "streamId", stream_id as f64)?;
                put_number(c, object, "parentStreamId", parent_stream_id as f64)?;
                let mut pairs = Vec::with_capacity(headers.len());
                for (name, value) in headers {
                    let name = c.string_rooted(&String::from_utf8_lossy(&name));
                    let value = c.string_rooted(&String::from_utf8_lossy(&value));
                    pairs.push(c.array_rooted(&[name, value])?);
                }
                let pairs = c.array_rooted(&pairs)?;
                put(c, object, "headers", pairs)?;
            }
            crate::modules::http2_native::H2Event::Data {
                stream_id,
                bytes,
                end_stream,
            } => {
                put_text(c, object, "type", "data")?;
                put_number(c, object, "streamId", stream_id as f64)?;
                put_bool(c, object, "endStream", end_stream)?;
                let bytes = c.uint8_array_rooted(&bytes)?;
                put(c, object, "data", bytes)?;
            }
            crate::modules::http2_native::H2Event::StreamClose {
                stream_id,
                error_code,
            } => {
                put_text(c, object, "type", "close")?;
                put_number(c, object, "streamId", stream_id as f64)?;
                put_number(c, object, "code", error_code as f64)?;
            }
        }
        values.push(object);
    }
    c.array_rooted(&values)
}

pub(crate) fn drain_outbound(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = number_arg(c, args, 0)?;
    let bytes =
        crate::modules::http2_native::drain_outbound(id).map_err(|error| h2_error(c, error))?;
    c.uint8_array_rooted(&bytes)
}

pub(crate) fn submit_headers(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = number_arg(c, args, 0)?;
    let stream_id = number_arg(c, args, 1)? as i32;
    let Some(headers) = args.get(2).copied() else {
        let error = c.type_error_rooted("HTTP/2 headers are required")?;
        return Err(c.throw(error));
    };
    let headers = header_pairs(c, headers)?;
    let end_stream = args
        .get(3)
        .is_some_and(|value| c.to_number(*value).unwrap_or(0.0) != 0.0);
    crate::modules::http2_native::submit_headers(id, stream_id, &headers, end_stream)
        .map_err(|error| h2_error(c, error))?;
    Ok(c.undefined())
}

pub(crate) fn submit_settings(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = number_arg(c, args, 0)?;
    let Some(settings) = args.get(1).copied() else {
        let error = c.type_error_rooted("HTTP/2 settings are required")?;
        return Err(c.throw(error));
    };
    let settings = settings_pairs(c, settings)?;
    crate::modules::http2_native::submit_settings(id, &settings)
        .map_err(|error| h2_error(c, error))?;
    Ok(c.undefined())
}

pub(crate) fn submit_data(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = number_arg(c, args, 0)?;
    let stream_id = number_arg(c, args, 1)? as i32;
    let Some(bytes) = args.get(2).and_then(|value| c.view_bytes_rooted(*value)) else {
        let error = c.type_error_rooted("HTTP/2 data must be a byte buffer")?;
        return Err(c.throw(error));
    };
    let end_stream = args
        .get(3)
        .is_some_and(|value| c.to_number(*value).unwrap_or(0.0) != 0.0);
    crate::modules::http2_native::submit_data(id, stream_id, &bytes, end_stream)
        .map_err(|error| h2_error(c, error))?;
    Ok(c.undefined())
}

pub(crate) fn submit_request(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = number_arg(c, args, 0)?;
    let Some(headers) = args.get(1).copied() else {
        let error = c.type_error_rooted("HTTP/2 headers are required")?;
        return Err(c.throw(error));
    };
    let headers = header_pairs(c, headers)?;
    let end_stream = args
        .get(2)
        .is_some_and(|value| c.to_number(*value).unwrap_or(0.0) != 0.0);
    match crate::modules::http2_native::submit_request(id, &headers, end_stream) {
        Ok(stream_id) => Ok(c.number(stream_id as f64)),
        Err(error) => Err(h2_error(c, error)),
    }
}

pub(crate) fn submit_push_promise(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = number_arg(c, args, 0)?;
    let parent_stream_id = number_arg(c, args, 1)? as i32;
    let Some(headers) = args.get(2).copied() else {
        let error = c.type_error_rooted("HTTP/2 push headers are required")?;
        return Err(c.throw(error));
    };
    let headers = header_pairs(c, headers)?;
    match crate::modules::http2_native::submit_push_promise(id, parent_stream_id, &headers) {
        Ok(stream_id) => Ok(c.number(stream_id as f64)),
        Err(error) => Err(h2_error(c, error)),
    }
}

pub(crate) fn close_session(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = number_arg(c, args, 0)?;
    crate::modules::http2_native::close_session(id);
    Ok(c.undefined())
}
