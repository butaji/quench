//! `Buffer.prototype.read*` / `write*` numeric accessors.
//!
//! One generic implementation (`read_num` / `write_num`) keyed by a
//! small spec struct; each Node method is a tiny macro-generated
//! trampoline so the dispatch table stays one line per capability.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::modules::buffer_enc as enc;
use crate::modules::buffer_methods::this_view;

#[derive(Clone, Copy)]
enum Kind {
    UInt,
    Int,
    Float,
    Double,
    BigUInt,
    BigInt,
}

#[derive(Clone, Copy)]
struct NumSpec {
    kind: Kind,
    size: usize,
    little_endian: bool,
    /// byteLength comes from arguments (readUIntLE family).
    dynamic_size: bool,
}

/// Validate a read/write `offset` argument: must be a number (or
/// undefined → 0); integral; in range. Node throws
/// `ERR_INVALID_ARG_TYPE` for non-numbers, `ERR_OUT_OF_RANGE` else.
fn offset_arg(arg: Option<&Value>, len: usize, size: usize) -> Result<usize, VmError> {
    offset_arg_opt(arg, len, size, true)
}

/// The `readUIntLE` family rejects an undefined offset; the
/// fixed-size methods default it to 0 (`allow_undefined`).
fn offset_arg_opt(
    arg: Option<&Value>,
    len: usize,
    size: usize,
    allow_undefined: bool,
) -> Result<usize, VmError> {
    match arg {
        None | Some(Value::Undefined) if allow_undefined => check_bounds(len, 0.0, size),
        Some(Value::Number(n)) => check_bounds(len, *n, size),
        Some(other) => Err(enc::invalid_arg_type(format!(
            "The \"offset\" argument must be of type number.{}",
            crate::modules::util::invalid_arg_received(other)
        ))),
        None => Err(enc::invalid_arg_type(
            "The \"offset\" argument must be of type number. Received undefined".to_string(),
        )),
    }
}

/// Node's `boundsError`: `length` is `len - size` (negative means
/// the read/write cannot fit at any offset → out-of-bounds error).
fn check_bounds(len: usize, offset: f64, size: usize) -> Result<usize, VmError> {
    if offset.is_nan() || (offset.is_finite() && offset.fract() != 0.0) {
        return Err(enc::out_of_range(
            "offset",
            "an integer",
            &enc::fmt_num(offset),
        ));
    }
    let length = len as i128 - size as i128;
    if length < 0 {
        return Err(enc::buffer_out_of_bounds(
            "Attempt to access memory outside buffer bounds",
        ));
    }
    if offset < 0.0 || offset > length as f64 {
        return Err(enc::out_of_range(
            "offset",
            &format!(">= 0 and <= {length}"),
            &enc::fmt_num(offset),
        ));
    }
    Ok(offset as usize)
}

/// Validate the `byteLength` argument of the `readUIntLE` family:
/// a number in 1..=6, `ERR_INVALID_ARG_TYPE` for non-numbers.
fn byte_length_arg(arg: Option<&Value>) -> Result<f64, VmError> {
    let n = match arg {
        Some(Value::Number(n)) => *n,
        other => {
            return Err(enc::invalid_arg_type(format!(
                "The \"byteLength\" argument must be of type number.{}",
                crate::modules::util::invalid_arg_received(other.unwrap_or(&Value::Undefined))
            )));
        }
    };
    if n.is_nan() || (n.is_finite() && n.fract() != 0.0) {
        return Err(enc::out_of_range(
            "byteLength",
            "an integer",
            &enc::fmt_num(n),
        ));
    }
    if !(1.0..=6.0).contains(&n) {
        return Err(enc::out_of_range(
            "byteLength",
            ">= 1 and <= 6",
            &enc::fmt_num(n),
        ));
    }
    Ok(n)
}

fn read_num(view_bytes: &[u8], offset: usize, spec: NumSpec) -> Value {
    let raw = &view_bytes[offset..offset + spec.size];
    let mut buf = [0u8; 8];
    let n = raw.len().min(8);
    if spec.little_endian {
        buf[..n].copy_from_slice(&raw[..n]);
    } else {
        buf[8 - n..].copy_from_slice(&raw[..n]);
    }
    let bits = if spec.little_endian {
        u64::from_le_bytes(buf)
    } else {
        u64::from_be_bytes(buf)
    };
    match spec.kind {
        Kind::Float => Value::Number(f64::from(f32::from_bits(bits as u32))),
        Kind::Double => Value::Number(f64::from_bits(bits)),
        Kind::BigUInt => Value::BigInt(bits.to_string()),
        Kind::BigInt => Value::BigInt((bits as i64).to_string()),
        Kind::UInt => Value::Number(bits as f64),
        Kind::Int => {
            let shift = 64 - 8 * spec.size as u32;
            Value::Number(((bits << shift) as i64 >> shift) as f64)
        }
    }
}

fn read_entry(receiver: Option<&Value>, args: &[Value], spec: NumSpec) -> Result<Value, VmError> {
    let view = this_view(receiver)?;
    let size = if spec.dynamic_size {
        let byte_length = byte_length_arg(args.get(1))?;
        let mut dynamic = spec;
        dynamic.size = byte_length as usize;
        let bytes = view.buffer.bytes.borrow();
        let slice = &bytes[view.byte_offset..view.byte_offset + view.length];
        let offset = offset_arg_opt(args.first(), slice.len(), dynamic.size, false)?;
        return Ok(read_num(slice, offset, dynamic));
    } else {
        spec.size
    };
    let bytes = view.buffer.bytes.borrow();
    let slice = &bytes[view.byte_offset..view.byte_offset + view.length];
    let offset = offset_arg(args.first(), slice.len(), size)?;
    Ok(read_num(slice, offset, spec))
}

fn write_entry(receiver: Option<&Value>, args: &[Value], spec: NumSpec) -> Result<Value, VmError> {
    let view = this_view(receiver)?;
    let size = if spec.dynamic_size {
        byte_length_arg(args.get(2))? as usize
    } else {
        spec.size
    };
    let bits = value_bits(args.first().unwrap_or(&Value::Undefined), spec.kind, size)?;
    let mut bytes = view.buffer.bytes.borrow_mut();
    let offset = offset_arg(args.get(1), view.length, size)?;
    let raw = if spec.little_endian {
        bits.to_le_bytes()
    } else {
        bits.to_be_bytes()
    };
    let slice = &mut bytes[view.byte_offset + offset..view.byte_offset + offset + size];
    if spec.little_endian {
        slice.copy_from_slice(&raw[..size]);
    } else {
        slice.copy_from_slice(&raw[8 - size..]);
    }
    Ok(Value::Number((offset + size) as f64))
}

fn value_bits(value: &Value, kind: Kind, size: usize) -> Result<u64, VmError> {
    match kind {
        Kind::Float | Kind::Double => {
            let Value::Number(n) = value else {
                return Err(enc::invalid_arg_type(format!(
                    "The \"value\" argument must be of type number.{}",
                    crate::modules::util::invalid_arg_received(value)
                )));
            };
            Ok(match kind {
                Kind::Float => (*n as f32).to_bits() as u64,
                _ => (*n).to_bits(),
            })
        }
        Kind::BigInt | Kind::BigUInt => bigint_bits(value, kind),
        Kind::UInt | Kind::Int => int_bits(value, kind, size),
    }
}

fn bigint_bits(value: &Value, kind: Kind) -> Result<u64, VmError> {
    let Value::BigInt(text) = value else {
        return Err(enc::invalid_arg_type(format!(
            "The \"value\" argument must be of type bigint.{}",
            crate::modules::util::invalid_arg_received(value)
        )));
    };
    let parsed: i128 = text.parse().unwrap_or(0);
    let (min, max) = match kind {
        Kind::BigInt => (i64::MIN as i128, i64::MAX as i128),
        _ => (0, u64::MAX as i128),
    };
    if parsed < min || parsed > max {
        // `checkInt` with byteLength > 3 uses the power form.
        let range = match kind {
            Kind::BigInt => ">= -(2n ** 63n) and < 2n ** 63n",
            _ => ">= 0n and < 2n ** 64n",
        };
        return Err(enc::out_of_range(
            "value",
            range,
            &format!("{}n", enc::separated(text)),
        ));
    }
    Ok(match kind {
        Kind::BigInt => parsed as i64 as u64,
        _ => parsed as u64,
    })
}

fn int_bits(value: &Value, kind: Kind, size: usize) -> Result<u64, VmError> {
    // `value = +value`: strings and booleans coerce.
    let n = match value {
        Value::Number(n) => *n,
        Value::Boolean(b) => f64::from(u8::from(*b)),
        Value::String(s) => s.trim().parse().unwrap_or(f64::NAN),
        _ => f64::NAN,
    };
    let bits = 8 * size as u32;
    let (min, max) = match kind {
        Kind::Int => (
            -((1u64 << (bits - 1)) as i64) as f64,
            ((1u64 << (bits - 1)) - 1) as f64,
        ),
        _ => (0.0, ((1u64 << bits) - 1) as f64),
    };
    if n < min || n > max {
        // `checkInt`: sizes over 4 bytes use the power form.
        let range = if size > 4 {
            match kind {
                Kind::Int => format!(">= -(2 ** {}) and < 2 ** {}", bits - 1, bits - 1),
                _ => format!(">= 0 and < 2 ** {bits}"),
            }
        } else {
            format!(">= {} and <= {}", enc::fmt_num(min), enc::fmt_num(max))
        };
        return Err(enc::out_of_range("value", &range, &enc::fmt_num(n)));
    }
    Ok(if n.is_nan() { 0 } else { n as i64 as u64 })
}

macro_rules! num_method {
    ($read:ident, $write:ident, $kind:expr, $size:expr, $le:expr, $dynamic:expr) => {
        pub fn $read(
            _state: &Rc<RefCell<HostState>>,
            receiver: Option<&Value>,
            args: &[Value],
        ) -> Result<Value, VmError> {
            read_entry(
                receiver,
                args,
                NumSpec {
                    kind: $kind,
                    size: $size,
                    little_endian: $le,
                    dynamic_size: $dynamic,
                },
            )
        }
        pub fn $write(
            _state: &Rc<RefCell<HostState>>,
            receiver: Option<&Value>,
            args: &[Value],
        ) -> Result<Value, VmError> {
            write_entry(
                receiver,
                args,
                NumSpec {
                    kind: $kind,
                    size: $size,
                    little_endian: $le,
                    dynamic_size: $dynamic,
                },
            )
        }
    };
}

num_method!(read_uint8, write_uint8, Kind::UInt, 1, true, false);
num_method!(read_uint16_le, write_uint16_le, Kind::UInt, 2, true, false);
num_method!(read_uint16_be, write_uint16_be, Kind::UInt, 2, false, false);
num_method!(read_uint32_le, write_uint32_le, Kind::UInt, 4, true, false);
num_method!(read_uint32_be, write_uint32_be, Kind::UInt, 4, false, false);
num_method!(read_int8, write_int8, Kind::Int, 1, true, false);
num_method!(read_int16_le, write_int16_le, Kind::Int, 2, true, false);
num_method!(read_int16_be, write_int16_be, Kind::Int, 2, false, false);
num_method!(read_int32_le, write_int32_le, Kind::Int, 4, true, false);
num_method!(read_int32_be, write_int32_be, Kind::Int, 4, false, false);
num_method!(read_float_le, write_float_le, Kind::Float, 4, true, false);
num_method!(read_float_be, write_float_be, Kind::Float, 4, false, false);
num_method!(
    read_double_le,
    write_double_le,
    Kind::Double,
    8,
    true,
    false
);
num_method!(
    read_double_be,
    write_double_be,
    Kind::Double,
    8,
    false,
    false
);
num_method!(
    read_bigint64_le,
    write_bigint64_le,
    Kind::BigInt,
    8,
    true,
    false
);
num_method!(
    read_bigint64_be,
    write_bigint64_be,
    Kind::BigInt,
    8,
    false,
    false
);
num_method!(
    read_biguint64_le,
    write_biguint64_le,
    Kind::BigUInt,
    8,
    true,
    false
);
num_method!(
    read_biguint64_be,
    write_biguint64_be,
    Kind::BigUInt,
    8,
    false,
    false
);
num_method!(read_uint_le, write_uint_le, Kind::UInt, 0, true, true);
num_method!(read_uint_be, write_uint_be, Kind::UInt, 0, false, true);
num_method!(read_int_le, write_int_le, Kind::Int, 0, true, true);
num_method!(read_int_be, write_int_be, Kind::Int, 0, false, true);
