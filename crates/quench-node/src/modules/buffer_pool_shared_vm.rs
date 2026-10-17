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
    c.get_named_property_rooted(o, n)
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

// Mirrors latest Node allocate/allocateAligned/createUnsafeAlignedBuffer policy.
pub(super) fn unsafe_allocate(
    c: &mut Context<'_>,
    data: RootId,
    size: RootId,
    alignment: Option<usize>,
    pooled: bool,
) -> Result<RootId, RootedError> {
    let amount = c.to_number(size)?;
    if amount <= 0.0 {
        return empty(c, data, &[]);
    }
    let pool = get(c, data, "pool")?;
    match alignment {
        None => {
            if pooled {
                let (buffer, from_pool) = array(c, data, size)?;
                if from_pool {
                    commit(c, data, size)?;
                }
                Ok(buffer)
            } else {
                empty(c, data, &[size])
            }
        }
        Some(alignment) => {
            if pooled && alignment <= POOL_ALIGNMENT && amount < limit(c, pool)? {
                let offset = get(c, pool, "offset")?;
                let padding = c.number((alignment - 1) as f64);
                let padded = c.arithmetic_rooted(NativeArithmetic::Add, offset, padding)?;
                let mask = c.number(-(alignment as f64));
                let offset = c.arithmetic_rooted(NativeArithmetic::BitwiseAnd, padded, mask)?;
                put(c, pool, "offset", offset)?;
                let buffer = reserve(c, data, pool, size, size)?;
                commit(c, data, size)?;
                return Ok(buffer);
            }
            // Core's backing allocations satisfy the typed-array base alignment.
            // Preserve a whole number of elements after the aligned view.
            let padding = c.number(alignment.max(SLICE_ALIGNMENT as usize) as f64);
            let length = c.arithmetic_rooted(NativeArithmetic::Add, size, padding)?;
            let ctor = get(c, pool, "ArrayBuffer")?;
            let backing = c.construct_rooted(ctor, ctor, &[length])?;
            let offset = c.array_buffer_aligned_offset_rooted(backing, alignment)?;
            let offset = c.number(offset as f64);
            empty(c, data, &[backing, offset, size])
        }
    }
}

const MAX_ALIGNMENT: usize = 1 << 30;
fn allocate_public(
    c: &mut Context<'_>,
    args: &[RootId],
    pooled: bool,
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let size = args.first().copied().unwrap_or_else(|| c.undefined());
    let name = c.string_rooted("size");
    let zero = c.number(0.0);
    let maximum = get(c, data, "maximumLength")?;
    let validator = get(c, data, "validateNumber")?;
    let undefined = c.undefined();
    c.call_rooted(validator, undefined, &[size, name, zero, maximum])?;
    let alignment = args.get(1).copied().unwrap_or_else(|| c.undefined());
    let alignment = if c.rooted_value(alignment).is_some_and(|v| v.is_undefined()) {
        None
    } else {
        let name = c.string_rooted("alignment");
        let one = c.number(1.0);
        let bound = c.number(MAX_ALIGNMENT as f64);
        let validator = get(c, data, "validateInteger")?;
        c.call_rooted(validator, undefined, &[alignment, name, one, bound])?;
        let n = c.to_number(alignment)? as usize;
        if !n.is_power_of_two() {
            let ctor = get(c, data, "ERR_INVALID_ARG_VALUE")?;
            let reason = c.string_rooted("must be a power of two");
            let error = c.construct_rooted(ctor, ctor, &[name, alignment, reason])?;
            return Err(c.throw(error));
        }
        let max = c.to_number(maximum)? - n.max(SLICE_ALIGNMENT as usize) as f64;
        if c.to_number(size)? > max {
            let ctor = get(c, data, "ERR_OUT_OF_RANGE")?;
            let name = c.string_rooted("size");
            let range = c.string_rooted(&format!("<= {max}"));
            let error = c.construct_rooted(ctor, ctor, &[name, range, size])?;
            return Err(c.throw(error));
        }
        Some(n)
    };
    unsafe_allocate(c, data, size, alignment, pooled)
}

pub(super) fn install_public_allocators(
    c: &mut Context<'_>,
    data: RootId,
    public: RootId,
) -> Result<(), RootedError> {
    let policy = super::buffer_policy_shared_vm::capture(c)?;
    c.set_prototype_rooted(data, policy)?;
    let buffer_name = c.string_rooted("Buffer");
    let view_name = c.string_rooted("Uint8Array");
    let types = c.array_rooted(&[buffer_name, view_name])?;
    put(c, data, "concatTypes", types)?;
    for (name, operation) in [
        ("allocUnsafe", "bufferAllocUnsafe"),
        ("allocUnsafeSlow", "bufferAllocUnsafeSlow"),
        ("concat", "bufferConcat"),
    ] {
        let f = c.host_constructor_with_data(crate::host::shared_vm::operation(operation), data)?;
        let key = c.string_rooted("name");
        let value = c.string_rooted(name);
        c.define_data_property_rooted(f, key, value, false, false, true)?;
        put(c, public, name, f)?;
    }
    Ok(())
}
pub(crate) fn alloc_unsafe(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    allocate_public(c, args, true)
}
pub(crate) fn alloc_unsafe_slow(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    allocate_public(c, args, false)
}

pub(crate) fn concat(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let list = args.first().copied().unwrap_or_else(|| c.undefined());
    let undefined = c.undefined();
    let name = c.string_rooted("list");
    let validate = get(c, data, "validateArray")?;
    c.call_rooted(validate, undefined, &[list, name])?;
    let count = get(c, list, "length")?;
    if c.rooted_value(count).and_then(|v| v.as_number()) == Some(0.0) {
        return empty(c, data, &[]);
    }
    let mut length = args.get(1).copied().unwrap_or_else(|| c.undefined());
    let explicit = !c.rooted_value(length).is_some_and(|v| v.is_undefined());
    if explicit {
        let validate = get(c, data, "validateInteger")?;
        let name = c.string_rooted("length");
        let zero = c.number(0.0);
        let maximum = get(c, data, "maximumLength")?;
        c.call_rooted(validate, undefined, &[length, name, zero, maximum])?;
    } else {
        length = c.number(0.0);
    }
    let byte_length = get(c, data, "byteLength")?;
    let mut index = 0.0;
    loop {
        let count = get(c, list, "length")?;
        let key = c.number(index);
        if !c.greater_than_rooted(count, key)? {
            break;
        }
        let value = c.get_property_rooted(list, key)?;
        if !c.has_builtin_brand_rooted(value, quench_runtime::BuiltinBrand::Uint8Array)? {
            let ctor = get(c, data, "ERR_INVALID_ARG_TYPE")?;
            let name = c.string_rooted(&format!("list[{index}]"));
            let types = get(c, data, "concatTypes")?;
            let error = c.construct_rooted(ctor, ctor, &[name, types, value])?;
            return Err(c.throw(error));
        }
        if !explicit {
            let bytes = c.call_rooted(byte_length, value, &[])?;
            length = c.arithmetic_rooted(NativeArithmetic::Add, length, bytes)?;
        }
        index += 1.0;
    }
    let buffer = unsafe_allocate(c, data, length, None, true)?;
    let requested = c.to_number(length)?;
    let set = get(c, data, "set")?;
    let mut position = 0.0;
    let mut index = 0.0;
    loop {
        let count = get(c, list, "length")?;
        let key = c.number(index);
        if !c.greater_than_rooted(count, key)? {
            break;
        }
        let mut value = c.get_property_rooted(list, key)?;
        let bytes = c.call_rooted(byte_length, value, &[])?;
        let bytes = c.to_number(bytes)?;
        let truncate = explicit && position + bytes > requested;
        if truncate {
            let subarray = get(c, data, "subarray")?;
            let zero = c.number(0.0);
            let remaining = c.number(requested - position);
            value = c.call_rooted(subarray, value, &[zero, remaining])?;
        }
        let offset = c.number(position);
        c.call_rooted(set, buffer, &[value, offset])?;
        position = if truncate {
            requested
        } else {
            position + bytes
        };
        if truncate {
            break;
        }
        index += 1.0;
    }
    if position < requested {
        let fill = get(c, data, "fill")?;
        let zero = c.number(0.0);
        let position = c.number(position);
        c.call_rooted(fill, buffer, &[zero, position, length])?;
    }
    Ok(buffer)
}
