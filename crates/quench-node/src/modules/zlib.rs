//! `zlib` module — real synchronous compression via `flate2`
//! (`crc32fast`/`miniz_oxide`). Each `*Sync` function accepts a Buffer /
//! TypedArray / string and returns a compressed (or decompressed) Buffer.

use std::cell::RefCell;
use std::io::{Read, Write};
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::value::Value;

use crate::host::HostState;

fn bytes_of(value: &Value) -> Result<Vec<u8>, VmError> {
    let bytes = match value {
        Value::String(s) => return Ok(s.as_bytes().to_vec()),
        Value::ArrayBuffer(buffer) => {
            if *buffer.detached.borrow() {
                return Err(execute::type_error("zlib: input ArrayBuffer is detached"));
            }
            buffer.bytes.borrow().clone()
        }
        Value::DataView(view) => {
            if *view.buffer.detached.borrow() {
                return Err(execute::type_error(
                    "zlib: input DataView buffer is detached",
                ));
            }
            view.buffer.bytes.borrow()[view.byte_offset..view.byte_offset + view.byte_length]
                .to_vec()
        }
        Value::Uint8Array(view) => {
            if *view.buffer.detached.borrow() {
                return Err(execute::type_error(
                    "zlib: input Uint8Array buffer is detached",
                ));
            }
            view.buffer.bytes.borrow()[view.byte_offset..view.byte_offset + view.length].to_vec()
        }
        other => {
            return Err(execute::type_error(&format!(
                "zlib: expected Buffer or string, got {}",
                crate::modules::util::invalid_arg_received(other)
            )))
        }
    };
    Ok(bytes)
}
fn output(bytes: Vec<u8>) -> Value {
    crate::modules::buffer_proto::make_buffer(&bytes)
}

fn compression_level(args: &[Value]) -> flate2::Compression {
    let Some(options) = args.get(1) else {
        return flate2::Compression::default();
    };
    let level = match quench_runtime::vm::get_property(options, "level") {
        Value::Number(n) if n.is_finite() => n.round().clamp(0.0, 9.0) as u32,
        _ => return flate2::Compression::default(),
    };
    flate2::Compression::new(level)
}

fn run<F>(args: &[Value], mut f: F) -> Result<Value, VmError>
where
    F: FnMut(&[u8], flate2::Compression) -> Result<Vec<u8>, std::io::Error>,
{
    let data = bytes_of(args.first().unwrap_or(&Value::Undefined))?;
    let out = f(&data, compression_level(args)).map_err(|e| execute::type_error(&e.to_string()))?;
    Ok(output(out))
}

fn gzip_deflate(data: &[u8], level: flate2::Compression) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::with_capacity(data.len() / 3), level);
    encoder.write_all(data)?;
    encoder.finish()
}

fn gzip_inflate(data: &[u8], _: flate2::Compression) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = flate2::bufread::GzDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

fn zlib_deflate(data: &[u8], level: flate2::Compression) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), level);
    encoder.write_all(data)?;
    encoder.finish()
}

fn zlib_inflate(data: &[u8], _: flate2::Compression) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = flate2::bufread::ZlibDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

fn raw_deflate(data: &[u8], level: flate2::Compression) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), level);
    encoder.write_all(data)?;
    encoder.finish()
}
fn raw_inflate(data: &[u8], _: flate2::Compression) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = flate2::bufread::DeflateDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}


pub fn gzip_value(args: &[Value]) -> Result<Value, VmError> {
    run(args, gzip_deflate)
}

pub fn gunzip_value(args: &[Value]) -> Result<Value, VmError> {
    run(args, gzip_inflate)
}

pub fn deflate_raw_value(args: &[Value]) -> Result<Value, VmError> {
    run(args, raw_deflate)
}

pub fn inflate_raw_value(args: &[Value]) -> Result<Value, VmError> {
    run(args, raw_inflate)
}

pub fn deflate_value(args: &[Value]) -> Result<Value, VmError> {
    run(args, zlib_deflate)
}

pub fn inflate_value(args: &[Value]) -> Result<Value, VmError> {
    run(args, zlib_inflate)
}

pub fn gzip(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = state;
    gzip_value(args)
}

pub fn gunzip(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = state;
    gunzip_value(args)
}

pub fn deflate_raw(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = state;
    deflate_raw_value(args)
}

pub fn inflate_raw(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = state;
    inflate_raw_value(args)
}

pub fn deflate(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = state;
    deflate_value(args)
}

pub fn inflate(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = state;
    inflate_value(args)
}

/// The `zlib` module namespace.
pub fn build() -> Value {
    crate::host::namespace_object(vec![
        (
            "gzipSync",
            crate::host::capability(crate::registry::SPEC_ZLIB_GZIP),
        ),
        (
            "gunzipSync",
            crate::host::capability(crate::registry::SPEC_ZLIB_GUNZIP),
        ),
        (
            "deflateRawSync",
            crate::host::capability(crate::registry::SPEC_ZLIB_DEFLATE_RAW),
        ),
        (
            "inflateRawSync",
            crate::host::capability(crate::registry::SPEC_ZLIB_INFLATE_RAW),
        ),
        (
            "deflateSync",
            crate::host::capability(crate::registry::SPEC_ZLIB_DEFLATE),
        ),
        (
            "inflateSync",
            crate::host::capability(crate::registry::SPEC_ZLIB_INFLATE),
        ),
        (
            "gzip",
            crate::host::capability(crate::registry::SPEC_ZLIB_GZIP),
        ),
        (
            "gunzip",
            crate::host::capability(crate::registry::SPEC_ZLIB_GUNZIP),
        ),
        (
            "deflateRaw",
            crate::host::capability(crate::registry::SPEC_ZLIB_DEFLATE_RAW),
        ),
        (
            "inflateRaw",
            crate::host::capability(crate::registry::SPEC_ZLIB_INFLATE_RAW),
        ),
        (
            "deflate",
            crate::host::capability(crate::registry::SPEC_ZLIB_DEFLATE),
        ),
        (
            "inflate",
            crate::host::capability(crate::registry::SPEC_ZLIB_INFLATE),
        ),
    ])
    .unwrap_or_else(|_| Value::Undefined)
}
