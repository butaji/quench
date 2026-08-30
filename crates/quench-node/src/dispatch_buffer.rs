//! Buffer capability dispatch — one table, one lookup. Capability
//! ids match `registry.rs` (`SPEC_BUFFER_*`, `SPEC_BUF_*`).

use crate::dispatch_handlers as handlers;
use crate::dispatch_handlers::CallHandler;
use crate::modules::buffer_methods as m;
use crate::modules::buffer_rw as rw;
use crate::modules::buffer_write as w;
use crate::registry::*;

const BUFFER_TABLE: &[(u16, CallHandler)] = &[
    (SPEC_BUFFER_FROM.cap, handlers::buffer_from),
    (SPEC_BUFFER_ALLOC.cap, handlers::buffer_alloc),
    (SPEC_BUFFER_BYTELENGTH.cap, handlers::buffer_byte_length),
    (SPEC_BUFFER_ISBUFFER.cap, handlers::buffer_is_buffer),
    (SPEC_BUFFER_CONCAT.cap, handlers::buffer_concat),
    (SPEC_BUFFER_NEW.cap, handlers::buffer_new),
    (SPEC_BUFFER_TOSTRING.cap, m::to_string),
    (SPEC_BUFFER_ALLOC_UNSAFE.cap, handlers::buffer_alloc_unsafe),
    (
        SPEC_BUFFER_ALLOC_UNSAFE_SLOW.cap,
        handlers::buffer_alloc_unsafe_slow,
    ),
    (SPEC_BUFFER_ISENCODING.cap, handlers::buffer_is_encoding),
    (SPEC_BUFFER_ISUTF8.cap, handlers::buffer_is_utf8),
    (SPEC_BUFFER_ISASCII.cap, handlers::buffer_is_ascii),
    (SPEC_BUFFER_COMPARE_STATIC.cap, m::compare_static),
    (SPEC_BUFFER_EQUALS.cap, m::equals),
    (SPEC_BUFFER_COMPARE.cap, m::compare),
    (SPEC_BUFFER_COPY.cap, m::copy),
    (SPEC_BUFFER_FILL.cap, m::fill),
    (SPEC_BUFFER_SLICE.cap, m::slice),
    (SPEC_BUFFER_SUBARRAY.cap, m::subarray),
    (SPEC_BUFFER_SWAP16.cap, m::swap16),
    (SPEC_BUFFER_SWAP32.cap, m::swap32),
    (SPEC_BUFFER_SWAP64.cap, m::swap64),
    (SPEC_BUFFER_TOJSON.cap, m::to_json),
    (SPEC_BUFFER_INDEX_OF.cap, m::index_of),
    (SPEC_BUFFER_LAST_INDEX_OF.cap, m::last_index_of),
    (SPEC_BUFFER_INCLUDES.cap, m::includes),
    (SPEC_BUFFER_WRITE.cap, w::write),
    (SPEC_BUFFER_INSPECT.cap, m::inspect),
    (SPEC_BUFFER_COPY_BYTES_FROM.cap, |state, _receiver, args| {
        crate::modules::buffer_from::copy_bytes_from(state, args)
    }),
    (
        SPEC_BUFFER_INSPECT_MAX_BYTES_GET.cap,
        crate::modules::buffer::inspect_max_bytes_get,
    ),
    (
        SPEC_BUFFER_INSPECT_MAX_BYTES_SET.cap,
        crate::modules::buffer::inspect_max_bytes_set,
    ),
    (SPEC_BUFFER_ASCII_WRITE.cap, w::ascii_write),
    (SPEC_BUFFER_LATIN1_WRITE.cap, w::latin1_write),
    (SPEC_BUFFER_UTF8_WRITE.cap, w::utf8_write),
    (SPEC_INTERNAL_BUFFER_UTF8_WRITE.cap, w::internal_utf8_write),
    (SPEC_TEXT_ENCODER_NEW.cap, |state, _receiver, args| {
        crate::modules::text_encoder::new_text_encoder(state, args)
    }),
    (SPEC_TEXT_ENCODER_ENCODE.cap, |state, receiver, args| {
        crate::modules::text_encoder::encode(state, receiver, args)
    }),
    (
        SPEC_TEXT_ENCODER_ENCODE_INTO.cap,
        |state, receiver, args| crate::modules::text_encoder::encode_into(state, receiver, args),
    ),
    (SPEC_BUF_READ_UINT8.cap, rw::read_uint8),
    (SPEC_BUF_WRITE_UINT8.cap, rw::write_uint8),
    (SPEC_BUF_READ_UINT16_LE.cap, rw::read_uint16_le),
    (SPEC_BUF_WRITE_UINT16_LE.cap, rw::write_uint16_le),
    (SPEC_BUF_READ_UINT16_BE.cap, rw::read_uint16_be),
    (SPEC_BUF_WRITE_UINT16_BE.cap, rw::write_uint16_be),
    (SPEC_BUF_READ_UINT32_LE.cap, rw::read_uint32_le),
    (SPEC_BUF_WRITE_UINT32_LE.cap, rw::write_uint32_le),
    (SPEC_BUF_READ_UINT32_BE.cap, rw::read_uint32_be),
    (SPEC_BUF_WRITE_UINT32_BE.cap, rw::write_uint32_be),
    (SPEC_BUF_READ_INT8.cap, rw::read_int8),
    (SPEC_BUF_WRITE_INT8.cap, rw::write_int8),
    (SPEC_BUF_READ_INT16_LE.cap, rw::read_int16_le),
    (SPEC_BUF_WRITE_INT16_LE.cap, rw::write_int16_le),
    (SPEC_BUF_READ_INT16_BE.cap, rw::read_int16_be),
    (SPEC_BUF_WRITE_INT16_BE.cap, rw::write_int16_be),
    (SPEC_BUF_READ_INT32_LE.cap, rw::read_int32_le),
    (SPEC_BUF_WRITE_INT32_LE.cap, rw::write_int32_le),
    (SPEC_BUF_READ_INT32_BE.cap, rw::read_int32_be),
    (SPEC_BUF_WRITE_INT32_BE.cap, rw::write_int32_be),
    (SPEC_BUF_READ_FLOAT_LE.cap, rw::read_float_le),
    (SPEC_BUF_WRITE_FLOAT_LE.cap, rw::write_float_le),
    (SPEC_BUF_READ_FLOAT_BE.cap, rw::read_float_be),
    (SPEC_BUF_WRITE_FLOAT_BE.cap, rw::write_float_be),
    (SPEC_BUF_READ_DOUBLE_LE.cap, rw::read_double_le),
    (SPEC_BUF_WRITE_DOUBLE_LE.cap, rw::write_double_le),
    (SPEC_BUF_READ_DOUBLE_BE.cap, rw::read_double_be),
    (SPEC_BUF_WRITE_DOUBLE_BE.cap, rw::write_double_be),
    (SPEC_BUF_READ_BIGINT64_LE.cap, rw::read_bigint64_le),
    (SPEC_BUF_WRITE_BIGINT64_LE.cap, rw::write_bigint64_le),
    (SPEC_BUF_READ_BIGINT64_BE.cap, rw::read_bigint64_be),
    (SPEC_BUF_WRITE_BIGINT64_BE.cap, rw::write_bigint64_be),
    (SPEC_BUF_READ_BIGUINT64_LE.cap, rw::read_biguint64_le),
    (SPEC_BUF_WRITE_BIGUINT64_LE.cap, rw::write_biguint64_le),
    (SPEC_BUF_READ_BIGUINT64_BE.cap, rw::read_biguint64_be),
    (SPEC_BUF_WRITE_BIGUINT64_BE.cap, rw::write_biguint64_be),
    (SPEC_BUF_READ_UINT_LE.cap, rw::read_uint_le),
    (SPEC_BUF_WRITE_UINT_LE.cap, rw::write_uint_le),
    (SPEC_BUF_READ_UINT_BE.cap, rw::read_uint_be),
    (SPEC_BUF_WRITE_UINT_BE.cap, rw::write_uint_be),
    (SPEC_BUF_READ_INT_LE.cap, rw::read_int_le),
    (SPEC_BUF_WRITE_INT_LE.cap, rw::write_int_le),
    (SPEC_BUF_READ_INT_BE.cap, rw::read_int_be),
    (SPEC_BUF_WRITE_INT_BE.cap, rw::write_int_be),
];

/// Resolve a buffer capability id to its handler, if it is one.
pub fn buffer_dispatch(cap: u16) -> Option<CallHandler> {
    BUFFER_TABLE
        .iter()
        .find(|(id, _)| *id == cap)
        .map(|(_, handler)| *handler)
}
