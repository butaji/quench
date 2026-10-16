//! Rust-owned allocation policy over real aligned, non-transferable Buffer pools.
use crate::host::NodeHost;
use quench_runtime::{NativeArithmetic, NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
pub(super) const DEFAULT_POOL_SIZE: usize = 64 * 1024;
const POOL_ALIGNMENT: usize = 64;
const SLICE_ALIGNMENT: i32 = 8;
// Node reserves a conservative UTF-8 upper bound before encoding a string.
const UTF8_BYTES_PER_SCALAR_BOUND: usize = 4;
fn get(c: &mut Context<'_>, o: RootId, n: &str) -> Result<RootId, RootedError> {
    let k = c.string_rooted(n);
    c.get_property_rooted(o, k)
}
fn put(c: &mut Context<'_>, o: RootId, n: &str, v: RootId) -> Result<(), RootedError> {
    let k = c.string_rooted(n);
    c.define_data_property_rooted(o, k, v, true, true, true)?;
    Ok(())
}
fn empty(c: &mut Context<'_>, data: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let f = get(c, data, "fast")?;
    c.construct_rooted(f, f, args)
}
pub(super) fn install(
    c: &mut Context<'_>,
    data: RootId,
    public: RootId,
) -> Result<(), RootedError> {
    let pool = c.null_object_rooted()?;
    put(c, pool, "public", public)?;
    let global = c.global_root()?;
    let ctor = get(c, global, "ArrayBuffer")?;
    put(c, pool, "ArrayBuffer", ctor)?;
    put(c, data, "pool", pool)?;
    create(c, pool)
}
fn create(c: &mut Context<'_>, pool: RootId) -> Result<(), RootedError> {
    let public = get(c, pool, "public")?;
    let size = get(c, public, "poolSize")?;
    let padding = c.number(POOL_ALIGNMENT as f64);
    let total = c.arithmetic_rooted(NativeArithmetic::Add, size, padding)?;
    let ctor = get(c, pool, "ArrayBuffer")?;
    let buffer = c.construct_rooted(ctor, ctor, &[total])?;
    let base = c.array_buffer_aligned_offset_rooted(buffer, POOL_ALIGNMENT)?;
    c.mark_untransferable_rooted(buffer)?;
    put(c, pool, "backing", buffer)?;
    put(c, pool, "size", size)?;
    let base = c.number(base as f64);
    put(c, pool, "base", base)?;
    let zero = c.number(0.0);
    put(c, pool, "offset", zero)
}
fn limit(c: &mut Context<'_>, pool: RootId) -> Result<f64, RootedError> {
    let public = get(c, pool, "public")?;
    let size = get(c, public, "poolSize")?;
    let mask = c.number(u32::MAX as f64);
    let bits = c.arithmetic_rooted(NativeArithmetic::BitwiseAnd, size, mask)?;
    Ok(((c.to_number(bits)? as i32 as u32) >> 1) as f64)
}
fn reserve(
    c: &mut Context<'_>,
    data: RootId,
    pool: RootId,
    length: RootId,
    reservation: RootId,
) -> Result<RootId, RootedError> {
    let size = get(c, pool, "size")?;
    let offset = get(c, pool, "offset")?;
    let remaining = c.subtract_rooted(size, offset)?;
    if c.greater_than_rooted(reservation, remaining)? {
        create(c, pool)?;
    }
    let buffer = get(c, pool, "backing")?;
    let base = get(c, pool, "base")?;
    let offset = get(c, pool, "offset")?;
    let begin = c.arithmetic_rooted(NativeArithmetic::Add, base, offset)?;
    empty(c, data, &[buffer, begin, length])
}
pub(super) fn commit(c: &mut Context<'_>, data: RootId, length: RootId) -> Result<(), RootedError> {
    let pool = get(c, data, "pool")?;
    let offset = get(c, pool, "offset")?;
    let mut offset = c.arithmetic_rooted(NativeArithmetic::Add, offset, length)?;
    let mask = c.number((SLICE_ALIGNMENT - 1) as f64);
    let bits = c.arithmetic_rooted(NativeArithmetic::BitwiseAnd, offset, mask)?;
    if c.to_number(bits)? != 0.0 {
        let clear = c.number((-SLICE_ALIGNMENT) as f64);
        let rounded = c.arithmetic_rooted(NativeArithmetic::BitwiseAnd, offset, clear)?;
        let step = c.number(SLICE_ALIGNMENT as f64);
        offset = c.arithmetic_rooted(NativeArithmetic::Add, rounded, step)?;
    }
    put(c, pool, "offset", offset)
}
pub(super) fn string(
    c: &mut Context<'_>,
    data: RootId,
    units: usize,
    bytes: usize,
) -> Result<(RootId, bool), RootedError> {
    let pool = get(c, data, "pool")?;
    let maximum = limit(c, pool)?;
    let actual = c.number(bytes as f64);
    // Node first tests the minimum UTF-16 length, then its four-byte bound,
    // falling back to the actual encoded length before selecting a pool.
    let bound = (units as f64) * UTF8_BYTES_PER_SCALAR_BOUND as f64;
    let reservation = if bound < maximum { bound } else { bytes as f64 };
    if units as f64 >= maximum || reservation >= maximum {
        return Ok((empty(c, data, &[actual])?, false));
    }
    let reservation = c.number(reservation);
    Ok((reserve(c, data, pool, actual, reservation)?, true))
}
pub(super) fn array(
    c: &mut Context<'_>,
    data: RootId,
    length: RootId,
) -> Result<(RootId, bool), RootedError> {
    let pool = get(c, data, "pool")?;
    let maximum = limit(c, pool)?;
    if c.to_number(length)? < maximum {
        Ok((reserve(c, data, pool, length, length)?, true))
    } else {
        Ok((empty(c, data, &[length])?, false))
    }
}
