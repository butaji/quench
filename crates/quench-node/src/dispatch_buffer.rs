//! Buffer capability dispatch — one table, one lookup. Capability
//! ids match `registry.rs` (`SPEC_BUFFER_*`, `SPEC_BUF_*`).

use crate::dispatch_handlers as handlers;
use crate::dispatch_handlers::CallHandler;
use crate::modules::buffer_methods as m;
use crate::modules::buffer_rw as rw;
use crate::modules::buffer_write as w;

const BUFFER_TABLE: &[(u16, CallHandler)] = &[
    (0x0800, handlers::buffer_from),
    (0x0801, handlers::buffer_alloc),
    (0x0802, handlers::buffer_byte_length),
    (0x0803, handlers::buffer_is_buffer),
    (0x0804, handlers::buffer_concat),
    (0x0805, handlers::buffer_new),
    (0x0808, m::to_string),
    (0x080B, handlers::buffer_alloc_unsafe),
    (0x080C, handlers::buffer_alloc_unsafe),
    (0x080D, handlers::buffer_is_encoding),
    (0x080E, handlers::buffer_is_utf8),
    (0x080F, handlers::buffer_is_ascii),
    (0x0810, m::compare_static),
    (0x0811, m::equals),
    (0x0812, m::compare),
    (0x0813, m::copy),
    (0x0814, m::fill),
    (0x0815, m::slice),
    (0x0820, m::subarray),
    (0x0816, m::swap16),
    (0x0817, m::swap32),
    (0x0818, m::swap64),
    (0x0819, m::to_json),
    (0x081A, m::index_of),
    (0x081B, m::last_index_of),
    (0x081C, m::includes),
    (0x081D, w::write),
    (0x081E, m::inspect),
    (0x081F, |state, _receiver, args| {
        crate::modules::buffer_from::copy_bytes_from(state, args)
    }),
    (0x0850, w::ascii_write),
    (0x0851, w::latin1_write),
    (0x0852, w::utf8_write),
    (0x084C, |state, _receiver, args| {
        crate::modules::text_encoder::new_text_encoder(state, args)
    }),
    (0x084D, |state, receiver, args| {
        crate::modules::text_encoder::encode(state, receiver, args)
    }),
    (0x084E, |state, receiver, args| {
        crate::modules::text_encoder::encode_into(state, receiver, args)
    }),
    (0x0820, rw::read_uint8),
    (0x0821, rw::write_uint8),
    (0x0822, rw::read_uint16_le),
    (0x0823, rw::write_uint16_le),
    (0x0824, rw::read_uint16_be),
    (0x0825, rw::write_uint16_be),
    (0x0826, rw::read_uint32_le),
    (0x0827, rw::write_uint32_le),
    (0x0828, rw::read_uint32_be),
    (0x0829, rw::write_uint32_be),
    (0x082A, rw::read_int8),
    (0x082B, rw::write_int8),
    (0x082C, rw::read_int16_le),
    (0x082D, rw::write_int16_le),
    (0x082E, rw::read_int16_be),
    (0x082F, rw::write_int16_be),
    (0x0830, rw::read_int32_le),
    (0x0831, rw::write_int32_le),
    (0x0832, rw::read_int32_be),
    (0x0833, rw::write_int32_be),
    (0x0834, rw::read_float_le),
    (0x0835, rw::write_float_le),
    (0x0836, rw::read_float_be),
    (0x0837, rw::write_float_be),
    (0x0838, rw::read_double_le),
    (0x0839, rw::write_double_le),
    (0x083A, rw::read_double_be),
    (0x083B, rw::write_double_be),
    (0x083C, rw::read_bigint64_le),
    (0x083D, rw::write_bigint64_le),
    (0x083E, rw::read_bigint64_be),
    (0x083F, rw::write_bigint64_be),
    (0x0840, rw::read_biguint64_le),
    (0x0841, rw::write_biguint64_le),
    (0x0842, rw::read_biguint64_be),
    (0x0843, rw::write_biguint64_be),
    (0x0844, rw::read_uint_le),
    (0x0845, rw::write_uint_le),
    (0x0846, rw::read_uint_be),
    (0x0847, rw::write_uint_be),
    (0x0848, rw::read_int_le),
    (0x0849, rw::write_int_le),
    (0x084A, rw::read_int_be),
    (0x084B, rw::write_int_be),
];

/// Resolve a buffer capability id to its handler, if it is one.
pub fn buffer_dispatch(cap: u16) -> Option<CallHandler> {
    BUFFER_TABLE
        .iter()
        .find(|(id, _)| *id == cap)
        .map(|(_, handler)| *handler)
}
