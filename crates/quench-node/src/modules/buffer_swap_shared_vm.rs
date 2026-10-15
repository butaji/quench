//! Buffer byte swapping over Core-owned typed-array views.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
// Node's short-view path swaps elements; its native path swaps physical bytes.
// The crossover is observable for generic typed arrays with multi-byte elements.
const SMALL_SWAP_MAX_ELEMENTS: usize = 32;
const SWAP64_NATIVE_MIN_ELEMENTS: usize = 48;
struct Swap {
    name: &'static str,
    width: usize,
    native_min: usize,
}
const SWAPS: [Swap; 3] = [
    Swap {
        name: "swap16",
        width: std::mem::size_of::<u16>(),
        native_min: SMALL_SWAP_MAX_ELEMENTS + 1,
    },
    Swap {
        name: "swap32",
        width: std::mem::size_of::<u32>(),
        native_min: SMALL_SWAP_MAX_ELEMENTS + 1,
    },
    Swap {
        name: "swap64",
        width: std::mem::size_of::<u64>(),
        native_min: SWAP64_NATIVE_MIN_ELEMENTS,
    },
];
fn get(context: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}
fn getter(context: &mut Context<'_>, prototype: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    let descriptor = context.own_property_descriptor_rooted(prototype, key)?;
    get(context, descriptor, "get")
}
pub(crate) fn install(context: &mut Context<'_>, prototype: RootId) -> Result<(), RootedError> {
    let global = context.global_root()?;
    let constructor = get(context, global, "Uint8Array")?;
    let byte_prototype = get(context, constructor, "prototype")?;
    let typed_prototype = context.prototype_rooted(byte_prototype)?;
    let length = getter(context, typed_prototype, "length")?;
    let buffer = getter(context, typed_prototype, "buffer")?;
    let offset = getter(context, typed_prototype, "byteOffset")?;
    let set = get(context, typed_prototype, "set")?;
    for (mode, swap) in SWAPS.iter().enumerate() {
        let mode = context.number(mode as f64);
        let data = context.array_rooted(&[mode, length, buffer, offset, constructor, set])?;
        let function = context
            .host_constructor_with_data(crate::host::shared_vm::operation("bufferSwap"), data)?;
        let name = context.string_rooted("name");
        let text = context.string_rooted(swap.name);
        context.define_data_property_rooted(function, name, text, false, false, true)?;
        let key = context.string_rooted(swap.name);
        context.define_data_property_rooted(prototype, key, function, true, true, true)?;
    }
    // Node exposes the same conversion function under both names.
    let to_string = get(context, prototype, "toString")?;
    let key = context.string_rooted("toLocaleString");
    context.define_data_property_rooted(prototype, key, to_string, true, true, true)?;
    Ok(())
}
fn invalid_size(context: &mut Context<'_>, width: usize) -> Result<RootedError, RootedError> {
    let error = context.range_error_rooted(&format!(
        "Buffer size must be a multiple of {}-bits",
        width * u8::BITS as usize
    ))?;
    let key = context.string_rooted("code");
    let code = context.string_rooted("ERR_INVALID_BUFFER_SIZE");
    context.set_property_rooted(error, key, code, error)?;
    Ok(context.throw(error))
}
pub(crate) fn operation(
    context: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let mode = get(context, data, "0")?;
    let mode = context
        .rooted_value(mode)
        .and_then(|value| value.as_number())
        .ok_or_else(|| RootedError::host("Buffer swap mode missing"))? as usize;
    let swap = SWAPS
        .get(mode)
        .ok_or_else(|| RootedError::host("Buffer swap mode invalid"))?;
    let length_getter = get(context, data, "1")?;
    let length = context.call_rooted(length_getter, receiver, &[])?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .ok_or_else(|| RootedError::host("typed-array intrinsic length is not numeric"))?
        as usize;
    if length % swap.width != 0 {
        return Err(invalid_size(context, swap.width)?);
    }
    if length < swap.native_min {
        for start in (0..length).step_by(swap.width) {
            for position in 0..swap.width / 2 {
                let left = context.number((start + position) as f64);
                let right = context.number((start + swap.width - 1 - position) as f64);
                let left_value = context.get_property_rooted(receiver, left)?;
                let right_value = context.get_property_rooted(receiver, right)?;
                context.set_property_rooted(receiver, left, right_value, receiver)?;
                context.set_property_rooted(receiver, right, left_value, receiver)?;
            }
        }
    } else {
        let mut bytes = context
            .view_bytes_rooted(receiver)
            .ok_or_else(|| RootedError::host("typed-array byte owner missing"))?;
        for unit in bytes.chunks_exact_mut(swap.width) {
            unit.reverse();
        }
        let buffer_getter = get(context, data, "2")?;
        let backing = context.call_rooted(buffer_getter, receiver, &[])?;
        let offset_getter = get(context, data, "3")?;
        let offset = context.call_rooted(offset_getter, receiver, &[])?;
        let constructor = get(context, data, "4")?;
        let byte_length = context.number(bytes.len() as f64);
        let destination =
            context.construct_rooted(constructor, constructor, &[backing, offset, byte_length])?;
        // A single Core bulk write bounds temporary roots independently of view length.
        let source = context.uint8_array_rooted(&bytes)?;
        let set = get(context, data, "5")?;
        context.call_rooted(set, destination, &[source])?;
    }
    Ok(receiver)
}
