//! The Math functions and the `Number.prototype` methods of the number family, each a standalone
//! `NativeFn` that `native_entry` reaches through the native table.

use super::native_table::NativeFn;
use super::number::{MATH_INLINE_ARGUMENTS, math_numbers, math_unary};
use super::*;
use smallvec::SmallVec;

/// The entry of each Math function, of `Math.initialize`, and of each `Number.prototype` method
/// the number family owns; `None` for any other native.
pub(super) fn entry(native: Native) -> Option<NativeFn> {
    match native {
        Native::MathInitialize => Some(math_initialize),
        Native::MathFloor | Native::MathAbs | Native::MathCeil | Native::MathRound | Native::MathTrunc | Native::MathSqrt
        | Native::MathFround | Native::MathSign | Native::MathClz32 | Native::MathF16Round | Native::MathAtanh
        | Native::MathAtan | Native::MathCbrt | Native::MathAcos | Native::MathAsin | Native::MathCos | Native::MathExp
        | Native::MathSin | Native::MathTan | Native::MathAcosh | Native::MathAsinh | Native::MathCosh
        | Native::MathExpm1 | Native::MathLog10 | Native::MathLog1p | Native::MathLog | Native::MathLog2
        | Native::MathSinh | Native::MathTanh => Some(math_unary_native),
        Native::MathPow => Some(math_pow),
        Native::MathAtan2 => Some(math_atan2),
        Native::MathMin | Native::MathMax => Some(math_extreme),
        Native::MathImul => Some(math_imul),
        Native::MathHypot => Some(math_hypot_native),
        Native::MathSumPrecise => Some(math_sum_precise_native),
        Native::MathRandom => Some(math_random),
        Native::NumberValueOf => Some(number_value_of),
        Native::NumberToLocaleString => Some(number_to_locale_string),
        Native::NumberString => Some(number_string),
        Native::NumberFixed | Native::NumberPrecision => Some(number_format_native),
        Native::NumberExponential => Some(number_exponential_native),
        _ => None,
    }
}

fn math_initialize(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.install_math(p).map(|()| Value::UNDEFINED)
}

/// A Math function of one Number: its first argument converted, then the one body (`math_unary`).
fn math_unary_native(vm: &mut Vm, p: &ResidualProgram, native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    let value = vm.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
    Ok(Value::number(math_unary(native, value).expect("a unary Math native")))
}

/// `Math.pow`: each argument converted in order, then the binary body.
fn math_pow(vm: &mut Vm, p: &ResidualProgram, native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    let x = vm.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
    let y = vm.to_number(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
    Ok(Value::number(math_numbers(native, &[x, y]).unwrap_or(f64::NAN)))
}

fn math_atan2(vm: &mut Vm, p: &ResidualProgram, native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    let y = args.first().copied().unwrap_or(Value::UNDEFINED);
    let x = args.get(1).copied().unwrap_or(Value::UNDEFINED);
    let numbers = [vm.to_number(p, y)?, vm.to_number(p, x)?];
    Ok(Value::number(math_numbers(native, &numbers).expect("Math.atan2")))
}

/// `Math.min` and `Math.max`: every argument converted in order, then the fold.
fn math_extreme(vm: &mut Vm, p: &ResidualProgram, native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    let mut numbers = SmallVec::<[f64; MATH_INLINE_ARGUMENTS]>::new();
    for value in args {
        numbers.push(vm.to_number(p, *value)?);
    }
    Ok(Value::number(math_numbers(native, &numbers).expect("Math.min or Math.max")))
}

fn math_imul(vm: &mut Vm, p: &ResidualProgram, native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    let left = vm.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
    let right = vm.to_number(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
    Ok(Value::number(math_numbers(native, &[left, right]).expect("Math.imul")))
}

fn math_hypot_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.math_hypot(p, args)
}

fn math_sum_precise_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.math_sum_precise(p, args.first().copied())
}

fn math_random(vm: &mut Vm, _p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    if crate::clock::input_edge_armed() {
        return Err(vm.capture_input_native(Native::MathRandom, this, args, None));
    }
    let mut state = vm.random_state;
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    vm.random_state = state;
    Ok(Value::number((state >> 11) as f64 / (1u64 << 53) as f64))
}

fn number_value_of(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    if this.as_number().is_some() {
        Ok(this)
    } else {
        vm.boxed_primitive_value(this).filter(|value| value.as_number().is_some()).ok_or_else(|| {
            vm.type_error(p, "Number.prototype.valueOf called on incompatible receiver".into())
        })
    }
}

fn number_to_locale_string(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    let number = vm.number_receiver_value(p, this)?;
    layer_hooks::intl_format_primitive(vm, p, Value::number(number), args)
}

fn number_string(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    let number = vm.number_receiver_value(p, this)?;
    let radix = match args.first().copied() {
        Some(value) if !value.is_undefined() => vm.to_number(p, value)? as u32,
        _ => 10,
    };
    if !(2..=36).contains(&radix) {
        return Err(vm.range_error(p, "Invalid radix".into()));
    }
    if radix == 10 || !number.is_finite() {
        return Ok(vm.heap.alloc_string(super::wtf16::JsString::from_number(number)));
    }
    Ok(vm.heap.alloc_string(super::string_extra::number_to_radix_string(number, radix)))
}

fn number_format_native(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.number_format(p, this, args, native)
}

fn number_exponential_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.number_exponential(p, this, args)
}
