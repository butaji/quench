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
    match value {
        Value::String(s) => Ok(s.as_bytes().to_vec()),
        Value::Uint8Array(view) => Ok(view.buffer.bytes.borrow()
            [view.byte_offset..view.byte_offset + view.length]
            .to_vec()),
        other => Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"buffer\" argument must be of type string or an instance of Buffer, TypedArray, DataView, or ArrayBuffer.{}",
            crate::modules::util::invalid_arg_received(other)
        ))),
    }
}

fn output(bytes: Vec<u8>) -> Value {
    crate::modules::buffer_proto::make_buffer(&bytes)
}

fn validate_window_bits(args: &[Value], minimum: u8) -> Result<(), VmError> {
    let Some(Value::Object(options)) = args.get(1) else {
        return Ok(());
    };
    let value = execute::get_property(&Value::Object(options.clone()), "windowBits");
    let Value::Number(window_bits) = value else {
        return Ok(());
    };
    if !window_bits.is_finite()
        || window_bits.fract() != 0.0
        || window_bits < f64::from(minimum)
        || window_bits > 15.0
    {
        return Err(crate::modules::buffer_enc::out_of_range(
            "options.windowBits",
            &format!(">= {minimum} and <= 15"),
            &execute::number_to_js_string(window_bits),
        ));
    }
    Ok(())
}

fn run<F>(args: &[Value], mut f: F) -> Result<Value, VmError>
where
    F: FnMut(&[u8]) -> Result<Vec<u8>, std::io::Error>,
{
    let data = bytes_of(args.first().unwrap_or(&Value::Undefined))?;
    let out = f(&data).map_err(|e| execute::type_error(&e.to_string()))?;
    Ok(output(out))
}

fn gzip_deflate(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = flate2::write::GzEncoder::new(
        Vec::with_capacity(data.len() / 3),
        flate2::Compression::default(),
    );
    encoder.write_all(data)?;
    encoder.finish()
}

fn gzip_inflate(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = flate2::bufread::GzDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

fn zlib_deflate(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data)?;
    encoder.finish()
}

fn zlib_inflate(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = flate2::bufread::ZlibDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

fn raw_deflate(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data)?;
    encoder.finish()
}

fn raw_inflate(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = flate2::bufread::DeflateDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

pub fn gzip(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = state;
    validate_window_bits(args, 9)?;
    run(args, gzip_deflate)
}

pub fn gunzip(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = state;
    run(args, gzip_inflate)
}

pub fn deflate_raw(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = state;
    run(args, raw_deflate)
}

pub fn inflate_raw(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = state;
    run(args, raw_inflate)
}

pub fn deflate(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = state;
    run(args, zlib_deflate)
}

pub fn inflate(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = state;
    run(args, zlib_inflate)
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
    ])
    .unwrap_or_else(|_| Value::Undefined)
}
