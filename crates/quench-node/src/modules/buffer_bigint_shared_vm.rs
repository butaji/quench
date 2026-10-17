//! Node's 64-bit Buffer integer policy over original Core values and indexed properties.
use crate::host::NodeHost;
use quench_runtime::{NativeArithmetic as Arithmetic, NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
const WIDTH: usize = std::mem::size_of::<u64>();
const HALF_BITS: u32 = u32::BITS;
struct Method {
    name: &'static str,
    read: bool,
    signed: bool,
    little: bool,
    facade: &'static str,
}
const METHODS: [Method; 8] = [
    Method {
        name: "readBigUInt64LE",
        read: true,
        signed: false,
        little: true,
        facade: quench_js_check::checked_js!(
            "op=>function readBigUInt64LE(offset=0){return op(this,offset);}"
        ),
    },
    Method {
        name: "readBigUInt64BE",
        read: true,
        signed: false,
        little: false,
        facade: quench_js_check::checked_js!(
            "op=>function readBigUInt64BE(offset=0){return op(this,offset);}"
        ),
    },
    Method {
        name: "readBigInt64LE",
        read: true,
        signed: true,
        little: true,
        facade: quench_js_check::checked_js!(
            "op=>function readBigInt64LE(offset=0){return op(this,offset);}"
        ),
    },
    Method {
        name: "readBigInt64BE",
        read: true,
        signed: true,
        little: false,
        facade: quench_js_check::checked_js!(
            "op=>function readBigInt64BE(offset=0){return op(this,offset);}"
        ),
    },
    Method {
        name: "writeBigUInt64LE",
        read: false,
        signed: false,
        little: true,
        facade: quench_js_check::checked_js!(
            "op=>function writeBigUInt64LE(value,offset=0){return op(this,value,offset);}"
        ),
    },
    Method {
        name: "writeBigUInt64BE",
        read: false,
        signed: false,
        little: false,
        facade: quench_js_check::checked_js!(
            "op=>function writeBigUInt64BE(value,offset=0){return op(this,value,offset);}"
        ),
    },
    Method {
        name: "writeBigInt64LE",
        read: false,
        signed: true,
        little: true,
        facade: quench_js_check::checked_js!(
            "op=>function writeBigInt64LE(value,offset=0){return op(this,value,offset);}"
        ),
    },
    Method {
        name: "writeBigInt64BE",
        read: false,
        signed: true,
        little: false,
        facade: quench_js_check::checked_js!(
            "op=>function writeBigInt64BE(value,offset=0){return op(this,value,offset);}"
        ),
    },
];
pub(crate) fn install(context: &mut Context<'_>, prototype: RootId) -> Result<(), RootedError> {
    let global = context.global_root()?;
    let bigint = get(context, global, "BigInt")?;
    for (index, method) in METHODS.iter().enumerate() {
        let mode = context.number(index as f64);
        let data = context.array_rooted(&[mode, bigint])?;
        let operation = context
            .host_function_with_data(crate::host::shared_vm::operation("bufferBigInt"), data)?;
        let factory =
            context.evaluate_script_rooted(method.facade, "node:buffer/bigint-facade.js")?;
        let undefined = context.undefined();
        let function = context.call_rooted(factory, undefined, &[operation])?;
        for name in [method.name.to_owned(), method.name.replace("UInt", "Uint")] {
            let key = context.string_rooted(&name);
            context.define_data_property_rooted(prototype, key, function, true, true, true)?;
            if method.signed {
                break;
            }
        }
    }
    Ok(())
}
fn get(context: &mut Context<'_>, object: RootId, key: &str) -> Result<RootId, RootedError> {
    context.get_named_property_rooted(object, key)
}
fn index(context: &mut Context<'_>, object: RootId, offset: f64) -> Result<RootId, RootedError> {
    let key = context.number(offset);
    context.get_property_rooted(object, key)
}
fn absent(context: &Context<'_>, value: RootId) -> bool {
    context
        .rooted_value(value)
        .is_some_and(|value| value.is_undefined())
}
fn coded_range(
    context: &mut Context<'_>,
    message: &str,
    code: &str,
) -> Result<RootedError, RootedError> {
    let error = context.range_error_rooted(message)?;
    let key = context.string_rooted("code");
    let value = context.string_rooted(code);
    context.set_property_rooted(error, key, value, error)?;
    Ok(context.throw(error))
}
fn out_of_range(
    context: &mut Context<'_>,
    value: RootId,
    name: &str,
    range: &str,
) -> Result<RootedError, RootedError> {
    let received = super::util_shared_vm::range_error_received(context, value)?;
    coded_range(
        context,
        &format!(
            "The value of \"{name}\" is out of range. It must be {range}. Received {received}"
        ),
        "ERR_OUT_OF_RANGE",
    )
}
fn bounds(
    context: &mut Context<'_>,
    buffer: RootId,
    offset: RootId,
    read: bool,
) -> Result<(f64, RootId, RootId), RootedError> {
    let Some(number) = context
        .rooted_value(offset)
        .and_then(|value| value.as_number())
    else {
        return Err(super::util_shared_vm::argument_type_error(
            context, offset, "offset", "number",
        )?);
    };
    let first = index(context, buffer, number)?;
    // Readers capture both edge values. Writers' bounds test short-circuits.
    let last = if read || !absent(context, first) {
        index(context, buffer, number + (WIDTH - 1) as f64)?
    } else {
        context.undefined()
    };
    if absent(context, first) || absent(context, last) {
        let length = get(context, buffer, "length")?;
        let length = context.to_number(length)? - WIDTH as f64;
        if number.floor() != number {
            return Err(out_of_range(context, offset, "offset", "an integer")?);
        }
        if length < 0.0 {
            return Err(coded_range(
                context,
                "Attempt to access memory outside buffer bounds",
                "ERR_BUFFER_OUT_OF_BOUNDS",
            )?);
        }
        let length = context.number(length);
        let length = context.to_string(length)?;
        return Err(out_of_range(
            context,
            offset,
            "offset",
            &format!(">= 0 and <= {length}"),
        )?);
    }
    Ok((number, first, last))
}
fn product(
    context: &mut Context<'_>,
    value: RootId,
    power: u32,
    signed_top: bool,
) -> Result<RootId, RootedError> {
    let minus_one = context.number(-1.0);
    let value = if signed_top {
        context.arithmetic_rooted(Arithmetic::BitwiseAnd, value, minus_one)?
    } else {
        value
    };
    let factor = context.number((1u64 << power) as f64);
    let value = context.arithmetic_rooted(Arithmetic::Multiply, value, factor)?;
    if signed_top {
        context.arithmetic_rooted(Arithmetic::BitwiseAnd, value, minus_one)
    } else {
        Ok(value)
    }
}
fn half(
    context: &mut Context<'_>,
    buffer: RootId,
    offset: f64,
    first: RootId,
    last: RootId,
    high: bool,
    method: &Method,
) -> Result<RootId, RootedError> {
    let half_width = WIDTH / 2;
    let start = if high != method.little { 0 } else { half_width };
    let mut sum = None;
    for position in start..start + half_width {
        let value = if position == 0 {
            first
        } else if position == WIDTH - 1 {
            last
        } else {
            index(context, buffer, offset + position as f64)?
        };
        let significance = if method.little {
            position - start
        } else {
            half_width - 1 - (position - start)
        };
        let power = significance as u32 * u8::BITS;
        let term = if power == 0 {
            value
        } else {
            product(
                context,
                value,
                power,
                high && method.signed && significance == half_width - 1,
            )?
        };
        sum = Some(if let Some(sum) = sum {
            context.arithmetic_rooted(Arithmetic::Add, sum, term)?
        } else {
            term
        });
    }
    Ok(sum.expect("a half contains integer bytes"))
}
fn bigint(
    context: &mut Context<'_>,
    constructor: RootId,
    value: RootId,
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    context.call_rooted(constructor, undefined, &[value])
}
fn read(
    context: &mut Context<'_>,
    buffer: RootId,
    offset: RootId,
    constructor: RootId,
    method: &Method,
) -> Result<RootId, RootedError> {
    let (offset, first, last) = bounds(context, buffer, offset, true)?;
    let (high, low) = if method.signed {
        let high = half(context, buffer, offset, first, last, true, method)?;
        let high = bigint(context, constructor, high)?;
        let low = half(context, buffer, offset, first, last, false, method)?;
        (high, bigint(context, constructor, low)?)
    } else if method.little {
        let low = half(context, buffer, offset, first, last, false, method)?;
        let high = half(context, buffer, offset, first, last, true, method)?;
        let low = bigint(context, constructor, low)?;
        (bigint(context, constructor, high)?, low)
    } else {
        let high = half(context, buffer, offset, first, last, true, method)?;
        let low = half(context, buffer, offset, first, last, false, method)?;
        let high = bigint(context, constructor, high)?;
        (high, bigint(context, constructor, low)?)
    };
    let scale = context.bigint_rooted(1i128 << HALF_BITS);
    let high = context.arithmetic_rooted(Arithmetic::Multiply, high, scale)?;
    context.arithmetic_rooted(Arithmetic::Add, high, low)
}
/// Decode only a Core-produced, masked nonnegative BigInt payload, never guest text.
fn masked_bits(context: &mut Context<'_>, value: RootId, mask: i128) -> Result<u64, RootedError> {
    let mask = context.bigint_rooted(mask);
    let masked = context.arithmetic_rooted(Arithmetic::BitwiseAnd, value, mask)?;
    context
        .to_string(masked)?
        .parse::<u64>()
        .map_err(|_| RootedError::host("masked Core BigInt payload exceeded its byte layout"))
}
fn write(
    context: &mut Context<'_>,
    buffer: RootId,
    value: RootId,
    offset: RootId,
    method: &Method,
) -> Result<RootId, RootedError> {
    let (minimum, maximum, range) = if method.signed {
        (
            i64::MIN as i128,
            i64::MAX as i128,
            ">= -(2n ** 63n) and < 2n ** 63n",
        )
    } else {
        (0, u64::MAX as i128, ">= 0n and < 2n ** 64n")
    };
    let minimum = context.bigint_rooted(minimum);
    let maximum = context.bigint_rooted(maximum);
    if context.greater_than_rooted(value, maximum)?
        || context.greater_than_rooted(minimum, value)?
    {
        return Err(out_of_range(context, value, "value", range)?);
    }
    let (offset, _, _) = bounds(context, buffer, offset, false)?;
    for high in [false, true] {
        // The high half converts the original value after the first four writes.
        let raw = if high {
            masked_bits(context, value, u64::MAX as i128)? >> HALF_BITS
        } else {
            masked_bits(context, value, u32::MAX as i128)?
        };
        for byte in 0..WIDTH / 2 {
            let position = usize::from(high) * (WIDTH / 2) + byte;
            let position = if method.little {
                position
            } else {
                WIDTH - 1 - position
            };
            let number = if byte == 0 {
                raw as f64
            } else {
                ((raw as u32 as i32) >> (byte as u32 * u8::BITS)) as f64
            };
            let key = context.number(offset + position as f64);
            let value = context.number(number);
            context.set_property_rooted(buffer, key, value, buffer)?;
        }
    }
    Ok(context.number(offset + WIDTH as f64))
}
pub(crate) fn operation(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let mode = get(context, data, "0")?;
    let mode = context
        .rooted_value(mode)
        .and_then(|value| value.as_number())
        .ok_or_else(|| RootedError::host("Buffer integer method mode missing"))?
        as usize;
    let method = METHODS
        .get(mode)
        .ok_or_else(|| RootedError::host("Buffer integer method mode invalid"))?;
    let buffer = args.first().copied().unwrap_or_else(|| context.undefined());
    let offset = args
        .get(if method.read { 1 } else { 2 })
        .copied()
        .unwrap_or_else(|| context.undefined());
    let offset = if absent(context, offset) {
        context.number(0.0)
    } else {
        offset
    };
    if method.read {
        let constructor = get(context, data, "1")?;
        read(context, buffer, offset, constructor, method)
    } else {
        let value = args.get(1).copied().unwrap_or_else(|| context.undefined());
        write(context, buffer, value, offset, method)
    }
}
