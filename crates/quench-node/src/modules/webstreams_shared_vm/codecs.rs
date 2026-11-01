//! `TextEncoderStream`, `TextDecoderStream` (`lib/internal/webstreams/encoding.js`) and
//! `CompressionStream`, `DecompressionStream` over the zlib streams
//! (`lib/internal/webstreams/compression.js`).

use super::transform::{controller_enqueue, create_transform, side, stream_slots};
use super::{
    arg, attach, class_with, custom_inspect, get, install, is_undefined, key, literal, put, record, slots,
    this_state, throw, Brand, ClassMember, Context, Done, Outcome, WebError,
};
use crate::modules::host_members::{captured, current, function, members};
use quench_runtime::{RootId, RootedError};

members!(Member, "webCodecs", [
    EncoderConstruct,
    DecoderConstruct,
    CompressionConstruct,
    DecompressionConstruct,
    Encoding,
    Fatal,
    IgnoreBom,
    Readable,
    Writable,
    EncoderInspect,
    DecoderInspect,
    CompressionInspect,
    DecompressionInspect,
    EncodeTransform,
    EncodeFlush,
    DecodeTransform,
    DecodeFlush,
    DecompressionError,
]);

/// The classes' slots.
mod field {
    pub(super) const TRANSFORM: &str = "transform";
    pub(super) const READABLE: &str = "readable";
    pub(super) const WRITABLE: &str = "writable";
    pub(super) const PENDING: &str = "pendingHighSurrogate";
    pub(super) const HANDLE: &str = "handle";
}
use field::*;

/// The encoding `TextEncoderStream` produces.
const UTF8: &str = "utf-8";
/// U+FFFD REPLACEMENT CHARACTER, as a code unit and as UTF-8.
const REPLACEMENT: u16 = 0xFFFD;
const REPLACEMENT_UTF8: [u8; 3] = [0xEF, 0xBF, 0xBD];

/// The formats `CompressionFormat` names, with the zlib factories compressing and
/// decompressing each, and whether decompression rejects trailing garbage.
const FORMATS: [(&str, &str, &str, bool); 4] = [
    ("deflate", "createDeflate", "createInflate", true),
    ("deflate-raw", "createDeflateRaw", "createInflateRaw", false),
    ("gzip", "createGzip", "createGunzip", true),
    ("brotli", "createBrotliCompress", "createBrotliDecompress", false),
];

fn is_high_surrogate(unit: u16) -> bool {
    (0xD800..=0xDBFF).contains(&unit)
}

fn is_low_surrogate(unit: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&unit)
}

/// The encoder transform's chunk steps: well-formed UTF-16 across chunk boundaries.
fn encode(c: &mut Context<'_>, state: RootId, chunk: RootId, controller: RootId) -> Outcome {
    let text = c.to_string_code_units(chunk)?;
    let pending = get(c, state, PENDING)?;
    let mut pending = c.rooted_value(pending).and_then(|value| value.as_number()).map(|unit| unit as u16);
    let mut units = Vec::with_capacity(text.len() + 1);
    for unit in text {
        if let Some(high) = pending.take() {
            if is_low_surrogate(unit) {
                units.push(high);
                units.push(unit);
                continue;
            }
            units.push(REPLACEMENT);
        }
        if is_high_surrogate(unit) {
            pending = Some(unit);
        } else if is_low_surrogate(unit) {
            units.push(REPLACEMENT);
        } else {
            units.push(unit);
        }
    }
    let pending = match pending {
        Some(unit) => c.number(f64::from(unit)),
        None => c.undefined(),
    };
    put(c, state, PENDING, pending)?;
    if !units.is_empty() {
        let bytes = String::from_utf16_lossy(&units).into_bytes();
        let value = c.uint8_array_from_rooted(bytes)?;
        let controller = slots(c, controller)?;
        controller_enqueue(c, controller, value)?;
    }
    Ok(c.undefined())
}

fn encode_flush(c: &mut Context<'_>, state: RootId, controller: RootId) -> Outcome {
    let pending = get(c, state, PENDING)?;
    if !is_undefined(c, pending) {
        let value = c.uint8_array_rooted(&REPLACEMENT_UTF8)?;
        let controller = slots(c, controller)?;
        controller_enqueue(c, controller, value)?;
    }
    Ok(c.undefined())
}

fn decode(c: &mut Context<'_>, state: RootId, chunk: RootId, controller: RootId) -> Outcome {
    if is_undefined(c, chunk) {
        return Err(throw(c, WebError::ArgType("chunk", "string", chunk)));
    }
    let handle = get(c, state, HANDLE)?;
    let decode = get(c, handle, "decode")?;
    let options = literal(c, &[])?;
    let yes = c.boolean(true);
    put(c, options, "stream", yes)?;
    let value = c.call_rooted(decode, handle, &[chunk, options])?;
    if c.truthy_rooted(value)? {
        let controller = slots(c, controller)?;
        controller_enqueue(c, controller, value)?;
    }
    Ok(c.undefined())
}

fn decode_flush(c: &mut Context<'_>, state: RootId, controller: RootId) -> Outcome {
    let handle = get(c, state, HANDLE)?;
    let decode = get(c, handle, "decode")?;
    let value = c.call_rooted(decode, handle, &[])?;
    let controller_state = slots(c, controller)?;
    if c.truthy_rooted(value)? {
        controller_enqueue(c, controller_state, value)?;
    }
    let terminate = get(c, controller, "terminate")?;
    c.call_rooted(terminate, controller, &[])?;
    Ok(c.undefined())
}

/// A transform stream whose transform and flush are the class's codec steps.
fn codec_transform(c: &mut Context<'_>, state: RootId, transform: Member, flush: Member) -> Done {
    let transform = function(c, transform, "transform", &[state])?;
    let flush = function(c, flush, "flush", &[state])?;
    let classes = super::classes(c)?;
    let class = get(c, classes, Brand::TransformStream.name())?;
    let prototype = get(c, class, "prototype")?;
    let object = c.object_rooted()?;
    c.set_prototype_rooted(object, prototype)?;
    let undefined = c.undefined();
    create_transform(c, object, transform, flush, undefined)?;
    let stream = stream_slots(c, object)?;
    let readable = side(c, stream, true)?;
    let writable = side(c, stream, false)?;
    put(c, state, TRANSFORM, object)?;
    put(c, state, READABLE, readable)?;
    put(c, state, WRITABLE, writable)
}

/// `createEnumConverter('CompressionFormat', ...)` for the class `name`.
fn format(c: &mut Context<'_>, value: RootId, name: &str) -> Result<usize, RootedError> {
    let text = c.to_string(value)?;
    if let Some(index) = FORMATS.iter().position(|(format, ..)| *format == text) {
        return Ok(index);
    }
    let message = format!("Failed to construct '{name}': 1st argument value '{text}' is not a valid enum value of type CompressionFormat.");
    let error = c.type_error_rooted(&message)?;
    crate::modules::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_ARG_VALUE")?;
    Err(c.throw(error))
}

fn compression(c: &mut Context<'_>, this: RootId, args: &[RootId], brand: Brand) -> Outcome {
    let format_value = arg(c, args, 0);
    let index = format(c, format_value, brand.name())?;
    let (_, compress, decompress, strict) = FORMATS[index];
    let zlib = crate::host::shared_vm::commonjs::zlib_module(c)?;
    let decompressing = brand == Brand::DecompressionStream;
    let factory = get(c, zlib, if decompressing { decompress } else { compress })?;
    let options = if decompressing && strict {
        let options = literal(c, &[])?;
        let yes = c.boolean(true);
        put(c, options, "rejectGarbageAfterEnd", yes)?;
        options
    } else {
        c.undefined()
    };
    let handle = c.call_rooted(factory, zlib, &[options])?;
    let (readable, writable) = super::adapters::pair_from_duplex(c, handle)?;
    let state = record(c, &[(HANDLE, handle), (READABLE, readable), (WRITABLE, writable)])?;
    attach(c, this, brand, state)?;
    if decompressing {
        let listener = function(c, Member::DecompressionError, "", &[writable])?;
        let on = get(c, handle, "on")?;
        let event = c.string_rooted("error");
        c.call_rooted(on, handle, &[event, listener])?;
    }
    Ok(c.undefined())
}

const ENCODER_MEMBERS: [ClassMember<Member>; 3] = [
    ClassMember::Getter("encoding", Member::Encoding),
    ClassMember::Getter("readable", Member::Readable),
    ClassMember::Getter("writable", Member::Writable),
];

const DECODER_MEMBERS: [ClassMember<Member>; 5] = [
    ClassMember::Getter("encoding", Member::Encoding),
    ClassMember::Getter("fatal", Member::Fatal),
    ClassMember::Getter("ignoreBOM", Member::IgnoreBom),
    ClassMember::Getter("readable", Member::Readable),
    ClassMember::Getter("writable", Member::Writable),
];

const PAIR_MEMBERS: [ClassMember<Member>; 2] =
    [ClassMember::Getter("readable", Member::Readable), ClassMember::Getter("writable", Member::Writable)];

pub(super) fn build(c: &mut Context<'_>, table: RootId) -> Done {
    for (brand, construct, inspect, length, members, tagged) in [
        (Brand::TextEncoderStream, Member::EncoderConstruct, Member::EncoderInspect, 0, &ENCODER_MEMBERS[..], false),
        (Brand::TextDecoderStream, Member::DecoderConstruct, Member::DecoderInspect, 0, &DECODER_MEMBERS[..], false),
        (Brand::CompressionStream, Member::CompressionConstruct, Member::CompressionInspect, 1, &PAIR_MEMBERS[..], true),
        (Brand::DecompressionStream, Member::DecompressionConstruct, Member::DecompressionInspect, 1, &PAIR_MEMBERS[..], true),
    ] {
        let constructor = class_with(c, brand, construct, length, inspect, tagged)?;
        let prototype = get(c, constructor, "prototype")?;
        install(c, prototype, members)?;
        put(c, table, brand.name(), constructor)?;
    }
    Ok(())
}

/// The slots of a receiver of any of the four classes, with its brand.
fn codec(c: &mut Context<'_>, this: RootId) -> Result<(Brand, RootId), RootedError> {
    for brand in [Brand::TextEncoderStream, Brand::TextDecoderStream, Brand::CompressionStream] {
        if let Some(state) = super::state_of(c, this, brand)? {
            return Ok((brand, state));
        }
    }
    let state = this_state(c, this, Brand::DecompressionStream)?;
    Ok((Brand::DecompressionStream, state))
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let member: Member = current(c)?;
    match member {
        Member::EncoderConstruct => {
            let undefined = c.undefined();
            let state = record(c, &[(PENDING, undefined)])?;
            attach(c, this, Brand::TextEncoderStream, state)?;
            codec_transform(c, state, Member::EncodeTransform, Member::EncodeFlush)?;
            Ok(c.undefined())
        }
        Member::DecoderConstruct => {
            let decoder = super::intrinsic(c, key::TEXT_DECODER)?;
            let encoding = arg(c, args, 0);
            let encoding = if is_undefined(c, encoding) { c.string_rooted(UTF8) } else { encoding };
            let options = arg(c, args, 1);
            let handle = c.construct_rooted(decoder, decoder, &[encoding, options])?;
            let state = record(c, &[(HANDLE, handle)])?;
            attach(c, this, Brand::TextDecoderStream, state)?;
            codec_transform(c, state, Member::DecodeTransform, Member::DecodeFlush)?;
            Ok(c.undefined())
        }
        Member::CompressionConstruct => compression(c, this, args, Brand::CompressionStream),
        Member::DecompressionConstruct => compression(c, this, args, Brand::DecompressionStream),
        Member::Encoding => {
            let (brand, state) = codec(c, this)?;
            if brand == Brand::TextEncoderStream {
                return Ok(c.string_rooted(UTF8));
            }
            let handle = get(c, state, HANDLE)?;
            get(c, handle, "encoding")
        }
        Member::Fatal | Member::IgnoreBom => {
            let state = this_state(c, this, Brand::TextDecoderStream)?;
            let handle = get(c, state, HANDLE)?;
            get(c, handle, if member == Member::Fatal { "fatal" } else { "ignoreBOM" })
        }
        Member::Readable | Member::Writable => {
            let (_, state) = codec(c, this)?;
            get(c, state, if member == Member::Readable { READABLE } else { WRITABLE })
        }
        Member::EncoderInspect | Member::DecoderInspect | Member::CompressionInspect | Member::DecompressionInspect => {
            let (brand, state) = codec(c, this)?;
            let readable = get(c, state, READABLE)?;
            let writable = get(c, state, WRITABLE)?;
            let data = match brand {
                Brand::TextEncoderStream => {
                    let encoding = c.string_rooted(UTF8);
                    literal(c, &[("encoding", encoding), (READABLE, readable), (WRITABLE, writable)])?
                }
                Brand::TextDecoderStream => {
                    let handle = get(c, state, HANDLE)?;
                    let encoding = get(c, handle, "encoding")?;
                    let fatal = get(c, handle, "fatal")?;
                    let ignore_bom = get(c, handle, "ignoreBOM")?;
                    literal(c, &[("encoding", encoding), ("fatal", fatal), ("ignoreBOM", ignore_bom), (READABLE, readable), (WRITABLE, writable)])?
                }
                _ => literal(c, &[(READABLE, readable), (WRITABLE, writable)])?,
            };
            custom_inspect(c, this, brand.name(), data, args)
        }
        Member::EncodeTransform | Member::DecodeTransform => {
            let [state] = captured(c)?;
            let chunk = arg(c, args, 0);
            let controller = arg(c, args, 1);
            if member == Member::EncodeTransform {
                encode(c, state, chunk, controller)
            } else {
                decode(c, state, chunk, controller)
            }
        }
        Member::EncodeFlush | Member::DecodeFlush => {
            let [state] = captured(c)?;
            let controller = arg(c, args, 0);
            if member == Member::EncodeFlush {
                encode_flush(c, state, controller)
            } else {
                decode_flush(c, state, controller)
            }
        }
        Member::DecompressionError => {
            let [writable] = captured(c)?;
            let error = arg(c, args, 0);
            let stream = slots(c, writable)?;
            if !super::writable::is_locked(c, stream)? {
                super::writable::abort(c, stream, error)?;
            }
            Ok(c.undefined())
        }
    }
}
