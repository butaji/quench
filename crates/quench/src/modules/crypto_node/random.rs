//! Random bytes, integers, UUIDs and primes, and `timingSafeEqual`.

use super::errors::{self, CryptoError};
use super::{arg, callback_argument, complete, get, is_undefined, Context, Outcome};
use openssl::bn::{BigNum, BigNumContext};
use quench_runtime::{BuiltinBrand, RootId, RootedError, ValueType};

/// `kMaxPossibleLength`: the largest byte count a random operation fills.
const MAX_LENGTH: f64 = i32::MAX as f64;
/// `RAND_MAX` of `randomInt`: the integers six random bytes give.
const RANDOM_INT_MAX: u64 = (1 << RANDOM_INT_BITS) - 1;
const RANDOM_INT_BITS: u32 = 48;
const RANDOM_INT_BYTES: usize = (RANDOM_INT_BITS / u8::BITS) as usize;
/// The largest view `getRandomValues` fills.
const RANDOM_VALUES_MAX_BYTES: usize = 65_536;
use crate::modules::blob_shared_vm::UUID_VERSION_4;
const UUID_VERSION_MASK: u8 = 0x0f;
use crate::modules::blob_shared_vm::UUID_VARIANT_RFC4122;
const UUID_VARIANT_MASK: u8 = 0x3f;
use crate::modules::blob_shared_vm::UUID_BYTES;
use crate::modules::blob_shared_vm::UUID_VERSION_BYTE;
use crate::modules::blob_shared_vm::UUID_VARIANT_BYTE;
use crate::modules::blob_shared_vm::UUID_GROUP_ENDS;

pub(crate) fn fill(bytes: &mut [u8]) -> Result<(), openssl::error::ErrorStack> {
    openssl::rand::rand_bytes(bytes)
}

/// `validateNumber(value, name)` then Node's range check against `[0, maximum]`.
fn number_in_range(c: &mut Context<'_>, value: RootId, name: &str, maximum: f64) -> Result<f64, RootedError> {
    let Some(number) = c.rooted_value(value).and_then(|value| value.as_number()) else {
        return Err(errors::invalid_type(c, value, name, "of type number"));
    };
    if number.is_nan() || !(0.0..=maximum).contains(&number) {
        let range = format!(">= 0 && <= {maximum}");
        return Err(errors::out_of_range(c, name, &range, value));
    }
    Ok(number)
}

/// `assertOffset`, in bytes.
fn offset_argument(c: &mut Context<'_>, offset: RootId, element: usize, length: usize) -> Result<usize, RootedError> {
    let Some(number) = c.rooted_value(offset).and_then(|value| value.as_number()) else {
        return Err(errors::invalid_type(c, offset, "offset", "of type number"));
    };
    let bytes = number * element as f64;
    let maximum = (length as f64).min(MAX_LENGTH);
    if bytes.is_nan() || bytes > maximum || bytes < 0.0 {
        let bytes = c.number(bytes);
        return Err(errors::out_of_range(c, "offset", &format!(">= 0 && <= {maximum}"), bytes));
    }
    Ok(bytes as usize)
}

/// `assertSize`, in bytes.
fn size_argument(c: &mut Context<'_>, size: RootId, element: usize, offset: usize, length: f64) -> Result<usize, RootedError> {
    let Some(number) = c.rooted_value(size).and_then(|value| value.as_number()) else {
        return Err(errors::invalid_type(c, size, "size", "of type number"));
    };
    let bytes = number * element as f64;
    if bytes.is_nan() || !(0.0..=MAX_LENGTH).contains(&bytes) {
        let bytes = c.number(bytes);
        return Err(errors::out_of_range(c, "size", &format!(">= 0 && <= {MAX_LENGTH}"), bytes));
    }
    if bytes + offset as f64 > length {
        let total = c.number(bytes + offset as f64);
        return Err(errors::out_of_range(c, "size + offset", &format!("<= {length}"), total));
    }
    Ok(bytes as usize)
}

/// `crypto.randomBytes(size[, callback])`.
pub(crate) fn random_bytes(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (size, callback) = (arg(c, args, 0), arg(c, args, 1));
    let size = number_in_range(c, size, "size", MAX_LENGTH)? as usize;
    let callback = if is_undefined(c, callback) { None } else { Some(callback_argument(c, callback, "callback")?) };
    let mut bytes = vec![0; size];
    let filled = fill(&mut bytes).map_err(|stack| errors::openssl(c, stack));
    match callback {
        None => {
            filled?;
            super::buffer(c, bytes)
        }
        Some(callback) => {
            let result = filled.and_then(|()| super::buffer(c, bytes)).map(|buffer| vec![buffer]);
            complete(c, callback, result)
        }
    }
}

/// The element size and byte length of a fill target, or the `buf` type error.
fn fill_target(c: &mut Context<'_>, buffer: RootId) -> Result<(usize, usize), RootedError> {
    if !super::is_any_buffer(c, buffer)? && !super::is_view(c, buffer)? {
        return Err(errors::invalid_type(c, buffer, "buf", "an instance of ArrayBuffer or ArrayBufferView"));
    }
    let element = if c.has_builtin_brand_rooted(buffer, BuiltinBrand::TypedArray)? {
        let size = get(c, buffer, "BYTES_PER_ELEMENT")?;
        c.to_number(size)? as usize
    } else {
        1
    };
    let length = c.with_buffer_bytes_rooted(buffer, <[u8]>::len).unwrap_or(0);
    Ok((element, length))
}

fn fill_range(c: &mut Context<'_>, buffer: RootId, offset: usize, size: usize) -> Result<(), RootedError> {
    let filled = c.with_buffer_bytes_mut_rooted(buffer, |bytes| fill(&mut bytes[offset..offset + size]));
    match filled {
        Some(Ok(())) | None => Ok(()),
        Some(Err(stack)) => Err(errors::openssl(c, stack)),
    }
}

/// `crypto.randomFillSync(buf[, offset[, size]])`.
pub(crate) fn random_fill_sync(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (buffer, offset, size) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    let (element, length) = fill_target(c, buffer)?;
    let offset = if is_undefined(c, offset) { 0 } else { offset_argument(c, offset, element, length)? };
    let size = if is_undefined(c, size) { length - offset } else { size_argument(c, size, element, offset, length as f64)? };
    fill_range(c, buffer, offset, size)?;
    Ok(buffer)
}

/// `crypto.randomFill(buf[, offset[, size]], callback)`.
pub(crate) fn random_fill(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (buffer, offset, size, callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2), arg(c, args, 3));
    let (element, length) = fill_target(c, buffer)?;
    let (offset, size, callback) = if c.is_callable_rooted(offset)? {
        (0, length, offset)
    } else if c.is_callable_rooted(size)? {
        let offset = offset_argument(c, offset, element, length)?;
        (offset, length - offset, size)
    } else {
        let callback = callback_argument(c, callback, "callback")?;
        let offset = offset_argument(c, offset, element, length)?;
        let size = if is_undefined(c, size) { length - offset } else { size_argument(c, size, element, offset, length as f64)? };
        (offset, size, callback)
    };
    let result = fill_range(c, buffer, offset, size).map(|()| vec![buffer]);
    complete(c, callback, result)
}

fn safe_integer(c: &mut Context<'_>, value: RootId, name: &str) -> Result<f64, RootedError> {
    let safe = c.rooted_value(value).and_then(|value| value.as_number()).filter(|number| {
        number.fract() == 0.0 && number.abs() <= crate::modules::util_shared_vm::MAX_SAFE_INTEGER
    });
    safe.ok_or_else(|| errors::invalid_type(c, value, name, "a safe integer"))
}

/// A uniform integer in `[0, range)`, by rejection sampling six random bytes.
fn uniform(range: u64) -> Result<u64, openssl::error::ErrorStack> {
    let limit = RANDOM_INT_MAX - (RANDOM_INT_MAX % range);
    loop {
        let mut bytes = [0u8; RANDOM_INT_BYTES];
        fill(&mut bytes)?;
        let value = bytes.iter().fold(0u64, |value, byte| (value << u8::BITS) | u64::from(*byte));
        if value < limit {
            return Ok(value % range);
        }
    }
}

/// `crypto.randomInt([min, ]max[, callback])`.
pub(crate) fn random_int(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (mut min, mut max, mut callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    let min_unspecified = is_undefined(c, max) || c.is_callable_rooted(max)?;
    if min_unspecified {
        callback = max;
        max = min;
        min = c.number(0.0);
    }
    let callback = if is_undefined(c, callback) { None } else { Some(callback_argument(c, callback, "callback")?) };
    let low = safe_integer(c, min, "min")?;
    let high = safe_integer(c, max, "max")?;
    if high <= low {
        let range = format!("greater than the value of \"min\" ({})", c.to_string(min)?);
        return Err(errors::out_of_range(c, "max", &range, max));
    }
    let range = high - low;
    if range > RANDOM_INT_MAX as f64 {
        let name = if min_unspecified { "max" } else { "max - min" };
        let value = c.number(range);
        return Err(errors::out_of_range(c, name, &format!("<= {RANDOM_INT_MAX}"), value));
    }
    let value = uniform(range as u64).map(|offset| low + offset as f64).map_err(|stack| errors::openssl(c, stack));
    match callback {
        None => Ok(c.number(value?)),
        Some(callback) => {
            let result = value.map(|value| vec![c.number(value)]);
            super::complete_as(c, callback, result, super::Completion::NextTick)
        }
    }
}

/// `crypto.randomUUID([options])`: an RFC 4122 version 4 UUID.
pub(crate) fn random_uuid(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let options = arg(c, args, 0);
    if !is_undefined(c, options) {
        super::object_argument(c, options, "options")?;
        let cache = get(c, options, "disableEntropyCache")?;
        if !is_undefined(c, cache) && c.value_type_rooted(cache) != Some(ValueType::Boolean) {
            return Err(errors::invalid_type(c, cache, "options.disableEntropyCache", "of type boolean"));
        }
    }
    let mut bytes = [0u8; UUID_BYTES];
    fill(&mut bytes).map_err(|stack| errors::openssl(c, stack))?;
    bytes[UUID_VERSION_BYTE] = (bytes[UUID_VERSION_BYTE] & UUID_VERSION_MASK) | UUID_VERSION_4;
    bytes[UUID_VARIANT_BYTE] = (bytes[UUID_VARIANT_BYTE] & UUID_VARIANT_MASK) | UUID_VARIANT_RFC4122;
    let mut text = String::with_capacity(UUID_BYTES * 2 + UUID_GROUP_ENDS.len());
    for (index, byte) in bytes.iter().enumerate() {
        if UUID_GROUP_ENDS.contains(&index) {
            text.push('-');
        }
        text.push_str(&format!("{byte:02x}"));
    }
    Ok(c.string_rooted(&text))
}

/// The integer-element typed arrays `getRandomValues` accepts.
const INTEGER_ARRAYS: [BuiltinBrand; 9] = [
    BuiltinBrand::Int8Array,
    BuiltinBrand::Uint8Array,
    BuiltinBrand::Uint8ClampedArray,
    BuiltinBrand::Int16Array,
    BuiltinBrand::Uint16Array,
    BuiltinBrand::Int32Array,
    BuiltinBrand::Uint32Array,
    BuiltinBrand::BigInt64Array,
    BuiltinBrand::BigUint64Array,
];

/// `getRandomValues(typedArray)`, the receiver already checked.
pub(crate) fn random_values(c: &mut Context<'_>, array: RootId) -> Outcome {
    let mut integer = false;
    for brand in INTEGER_ARRAYS {
        integer = integer || c.has_builtin_brand_rooted(array, brand)?;
    }
    if !integer {
        return Err(errors::dom(c, "The data argument must be an integer-type TypedArray", "TypeMismatchError"));
    }
    let length = c.with_buffer_bytes_rooted(array, <[u8]>::len).unwrap_or(0);
    if length > RANDOM_VALUES_MAX_BYTES {
        return Err(errors::dom(c, "The requested length exceeds 65,536 bytes", "QuotaExceededError"));
    }
    fill_range(c, array, 0, length)?;
    Ok(array)
}

/// `crypto.getRandomValues(typedArray)` as the module exports it, needing no receiver.
pub(crate) fn get_random_values_unbound(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let array = arg(c, args, 0);
    random_values(c, array)
}

/// `crypto.timingSafeEqual(a, b)`.
pub(crate) fn timing_safe_equal(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let mut inputs = Vec::with_capacity(2);
    for (index, name) in ["buf1", "buf2"].into_iter().enumerate() {
        let value = arg(c, args, index);
        match super::buffer_bytes(c, value) {
            Some(bytes) => inputs.push(bytes),
            None => {
                let message = format!("The \"{name}\" argument must be an instance of ArrayBuffer, Buffer, TypedArray, or DataView.");
                return Err(errors::throw(c, CryptoError::InvalidArgTypeMessage(message)));
            }
        }
    }
    if inputs[0].len() != inputs[1].len() {
        return Err(errors::throw(c, CryptoError::TimingSafeEqualLength));
    }
    Ok(c.boolean(openssl::memcmp::eq(&inputs[0], &inputs[1])))
}

// ---- Primes --------------------------------------------------------------------------

/// A prime candidate: a bigint, or big-endian bytes.
fn candidate(c: &mut Context<'_>, value: RootId) -> Result<BigNum, RootedError> {
    if c.value_type_rooted(value) == Some(ValueType::BigInt) {
        let text = c.to_string(value)?;
        return BigNum::from_dec_str(&text).map_err(|stack| errors::openssl(c, stack));
    }
    match super::buffer_bytes(c, value) {
        Some(bytes) => BigNum::from_slice(&bytes).map_err(|stack| errors::openssl(c, stack)),
        None => Err(errors::invalid_type(
            c,
            value,
            "candidate",
            "of type bigint or an instance of ArrayBuffer, TypedArray, Buffer, or DataView",
        )),
    }
}

fn check_prime_value(c: &mut Context<'_>, args: &[RootId]) -> Result<bool, RootedError> {
    let (value, options) = (arg(c, args, 0), arg(c, args, 1));
    let number = candidate(c, value)?;
    let mut checks = 0;
    if !is_undefined(c, options) {
        super::object_argument(c, options, "options")?;
        let count = get(c, options, "checks")?;
        if !is_undefined(c, count) {
            checks = super::integer_argument(c, count, "options.checks", 0.0, i32::MAX as f64)? as i32;
        }
    }
    let mut context = BigNumContext::new().map_err(|stack| errors::openssl(c, stack))?;
    number.is_prime(checks, &mut context).map_err(|stack| errors::openssl(c, stack))
}

/// `crypto.checkPrimeSync(candidate[, options])`.
pub(crate) fn check_prime_sync(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let prime = check_prime_value(c, args)?;
    Ok(c.boolean(prime))
}

/// `crypto.checkPrime(candidate[, options], callback)`.
pub(crate) fn check_prime(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (callback, inputs) = trailing_callback(c, args)?;
    let result = check_prime_value(c, &inputs).map(|prime| vec![c.boolean(prime)]);
    super::complete_as(c, callback, result, super::Completion::JobUndefinedError)
}

/// The arguments before a trailing callback (an options argument may be omitted).
fn trailing_callback(c: &mut Context<'_>, args: &[RootId]) -> Result<(RootId, Vec<RootId>), RootedError> {
    let first = arg(c, args, 1);
    if c.is_callable_rooted(first)? {
        let undefined = c.undefined();
        return Ok((first, vec![arg(c, args, 0), undefined]));
    }
    let callback = arg(c, args, 2);
    let callback = callback_argument(c, callback, "callback")?;
    Ok((callback, vec![arg(c, args, 0), first]))
}

/// A `generatePrime` option that is a prime bound (`add`, `rem`).
fn prime_option(c: &mut Context<'_>, options: RootId, name: &str) -> Result<Option<BigNum>, RootedError> {
    let value = super::option(c, options, name)?;
    if is_undefined(c, value) {
        return Ok(None);
    }
    if c.value_type_rooted(value) == Some(ValueType::BigInt) {
        let text = c.to_string(value)?;
        return BigNum::from_dec_str(&text).map(Some).map_err(|stack| errors::openssl(c, stack));
    }
    match super::buffer_bytes(c, value) {
        Some(bytes) => BigNum::from_slice(&bytes).map(Some).map_err(|stack| errors::openssl(c, stack)),
        None => Err(errors::invalid_type(
            c,
            value,
            &format!("options.{name}"),
            "of type bigint or an instance of ArrayBuffer, TypedArray, Buffer, or DataView",
        )),
    }
}

fn boolean_option(c: &mut Context<'_>, options: RootId, name: &str) -> Result<bool, RootedError> {
    let value = super::option(c, options, name)?;
    if is_undefined(c, value) {
        return Ok(false);
    }
    if c.value_type_rooted(value) != Some(ValueType::Boolean) {
        return Err(errors::invalid_type(c, value, &format!("options.{name}"), "of type boolean"));
    }
    c.truthy_rooted(value)
}

fn generate_prime_value(c: &mut Context<'_>, args: &[RootId]) -> Outcome {
    let (size, options) = (arg(c, args, 0), arg(c, args, 1));
    let bits = super::integer_argument(c, size, "size", 1.0, MAX_LENGTH)? as i32;
    if !is_undefined(c, options) {
        super::object_argument(c, options, "options")?;
    }
    let safe = boolean_option(c, options, "safe")?;
    let bigint = boolean_option(c, options, "bigint")?;
    let add = prime_option(c, options, "add")?;
    let rem = prime_option(c, options, "rem")?;
    if rem.is_some() && add.is_none() {
        return Err(errors::throw(c, CryptoError::Plain("invalid options.rem")));
    }
    let mut prime = BigNum::new().map_err(|stack| errors::openssl(c, stack))?;
    let generated = prime.generate_prime(bits, safe, add.as_deref(), rem.as_deref());
    generated.map_err(|stack| errors::openssl(c, stack))?;
    let bytes = prime.to_vec();
    if !bigint {
        return super::array_buffer(c, bytes);
    }
    let global = c.global_root()?;
    let constructor = get(c, global, "BigInt")?;
    let text = c.string_rooted(&format!("0x{}", hex::encode(bytes)));
    let undefined = c.undefined();
    c.call_rooted(constructor, undefined, &[text])
}

/// `crypto.generatePrimeSync(size[, options])`.
pub(crate) fn generate_prime_sync(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    generate_prime_value(c, args)
}

/// `crypto.generatePrime(size[, options], callback)`.
pub(crate) fn generate_prime(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (callback, inputs) = trailing_callback(c, args)?;
    let result = generate_prime_value(c, &inputs).map(|prime| vec![prime]);
    super::complete_as(c, callback, result, super::Completion::JobUndefinedError)
}
