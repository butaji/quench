//! `node:punycode` in Rust: Punycode.js (RFC 3492 Bootstring), which lib/punycode.js is.
use crate::host::shared_vm::operation;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

/// Punycode.js's `version`.
const VERSION: &str = "2.1.0";
/// RFC 3492's Bootstring parameters for Punycode.
const BASE: u32 = 36;
const T_MIN: u32 = 1;
const T_MAX: u32 = 26;
const SKEW: u32 = 38;
const DAMP: u32 = 700;
const INITIAL_BIAS: u32 = 72;
const INITIAL_N: u32 = 0x80;
const DELIMITER: u16 = b'-' as u16;
/// The largest value Punycode.js's integers hold (`0x7FFFFFFF`).
const MAX_INT: u32 = i32::MAX as u32;
/// Code points at least this large are not basic.
const BASIC_LIMIT: u32 = 0x80;
/// The ACE prefix `toASCII` adds and `toUnicode` recognizes.
const ACE_PREFIX: &str = "xn--";
/// IDNA label separators `mapDomain` treats as a full stop (U+002E, U+3002, U+FF0E, U+FF61).
const LABEL_SEPARATORS: [u16; 4] = [0x2e, 0x3002, 0xff0e, 0xff61];
const FULL_STOP: u16 = 0x2e;
const AT_SIGN: u16 = b'@' as u16;
const DEPRECATION: (&str, &str) = ("The `punycode` module is deprecated. Please use a userland alternative instead.", "DEP0040");
/// How many frames `isInsideNodeModules` inspects for the deprecation.
const DEPRECATION_FRAMES: f64 = 100.0;

/// Punycode.js's `errors`.
enum Failure {
    Overflow,
    NotBasic,
    InvalidInput,
}

impl Failure {
    fn message(&self) -> &'static str {
        match self {
            Failure::Overflow => "Overflow: input needs wider integers to process",
            Failure::NotBasic => "Illegal input >= 0x80 (not a basic code point)",
            Failure::InvalidInput => "Invalid input",
        }
    }
}

fn adapt(delta: u32, points: u32, first: bool) -> u32 {
    let mut delta = if first { delta / DAMP } else { delta >> 1 };
    delta += delta / points;
    let mut k = 0;
    while delta > ((BASE - T_MIN) * T_MAX) >> 1 {
        delta /= BASE - T_MIN;
        k += BASE;
    }
    k + (BASE - T_MIN + 1) * delta / (delta + SKEW)
}

fn threshold(k: u32, bias: u32) -> u32 {
    if k <= bias {
        T_MIN
    } else if k >= bias + T_MAX {
        T_MAX
    } else {
        k - bias
    }
}

fn basic_to_digit(unit: u16) -> u32 {
    match unit {
        0x30..=0x39 => 26 + u32::from(unit - 0x30),
        0x41..=0x5a => u32::from(unit - 0x41),
        0x61..=0x7a => u32::from(unit - 0x61),
        _ => BASE,
    }
}

fn digit_to_basic(digit: u32) -> u16 {
    const LOWERCASE_A: u32 = b'a' as u32;
    const DIGIT_ZERO: u32 = b'0' as u32;
    const LETTERS: u32 = 26;
    (if digit < LETTERS { LOWERCASE_A + digit } else { DIGIT_ZERO + digit - LETTERS }) as u16
}

/// `ucs2decode`: code points, keeping lone surrogates.
fn ucs2_decode(units: &[u16]) -> Vec<u32> {
    char::decode_utf16(units.iter().copied())
        .map(|unit| unit.map_or_else(|lone| u32::from(lone.unpaired_surrogate()), u32::from))
        .collect()
}

fn ucs2_encode(points: &[u32]) -> Vec<u16> {
    let mut units = Vec::with_capacity(points.len());
    for &point in points {
        match char::from_u32(point) {
            Some(character) => units.extend(character.encode_utf16(&mut [0; 2]).iter()),
            None => units.push(point as u16),
        }
    }
    units
}

/// `punycode.decode`: the code points a Punycode string encodes.
fn decode_points(input: &[u16]) -> Result<Vec<u32>, Failure> {
    let basic = input.iter().rposition(|unit| *unit == DELIMITER).unwrap_or(0);
    let mut output = Vec::with_capacity(input.len());
    for &unit in &input[..basic] {
        if u32::from(unit) >= BASIC_LIMIT {
            return Err(Failure::NotBasic);
        }
        output.push(u32::from(unit));
    }
    let (mut i, mut n, mut bias) = (0u32, INITIAL_N, INITIAL_BIAS);
    let mut index = if basic > 0 { basic + 1 } else { 0 };
    while index < input.len() {
        let old = i;
        let mut w = 1u32;
        let mut k = BASE;
        loop {
            let unit = *input.get(index).ok_or(Failure::InvalidInput)?;
            index += 1;
            let digit = basic_to_digit(unit);
            if digit >= BASE {
                return Err(Failure::InvalidInput);
            }
            if digit > (MAX_INT - i) / w {
                return Err(Failure::Overflow);
            }
            i += digit * w;
            let t = threshold(k, bias);
            if digit < t {
                break;
            }
            if w > MAX_INT / (BASE - t) {
                return Err(Failure::Overflow);
            }
            w *= BASE - t;
            k += BASE;
        }
        let out = output.len() as u32 + 1;
        bias = adapt(i - old, out, old == 0);
        if i / out > MAX_INT - n {
            return Err(Failure::Overflow);
        }
        n += i / out;
        i %= out;
        output.insert(i as usize, n);
        i += 1;
    }
    Ok(output)
}

/// `punycode.encode`: the Punycode string of a string's code points.
fn encode_units(input: &[u16]) -> Result<Vec<u16>, Failure> {
    let points = ucs2_decode(input);
    let mut output: Vec<u16> = points.iter().filter(|point| **point < BASIC_LIMIT).map(|point| *point as u16).collect();
    let basic_length = output.len() as u32;
    let mut handled = basic_length;
    if basic_length > 0 {
        output.push(DELIMITER);
    }
    let (mut n, mut delta, mut bias) = (INITIAL_N, 0u32, INITIAL_BIAS);
    while (handled as usize) < points.len() {
        let m = points.iter().copied().filter(|point| *point >= n).min().unwrap_or(MAX_INT);
        let next = handled + 1;
        if m - n > (MAX_INT - delta) / next {
            return Err(Failure::Overflow);
        }
        delta += (m - n) * next;
        n = m;
        for &point in &points {
            if point < n {
                delta += 1;
                if delta > MAX_INT {
                    return Err(Failure::Overflow);
                }
            }
            if point == n {
                let mut q = delta;
                let mut k = BASE;
                loop {
                    let t = threshold(k, bias);
                    if q < t {
                        break;
                    }
                    output.push(digit_to_basic(t + (q - t) % (BASE - t)));
                    q = (q - t) / (BASE - t);
                    k += BASE;
                }
                output.push(digit_to_basic(q));
                bias = adapt(delta, handled + 1, handled == basic_length);
                delta = 0;
                handled += 1;
            }
        }
        delta += 1;
        n += 1;
    }
    Ok(output)
}

/// `mapDomain`: maps each label of a domain (after any `local@` part, which stays).
fn map_domain(input: &[u16], label: impl Fn(&[u16]) -> Result<Vec<u16>, Failure>) -> Result<Vec<u16>, Failure> {
    let mut parts = input.split(|unit| *unit == AT_SIGN);
    let first = parts.next().unwrap_or_default();
    let (mut result, domain) = match parts.next() {
        Some(domain) => {
            let mut local = first.to_vec();
            local.push(AT_SIGN);
            (local, domain)
        }
        None => (Vec::new(), first),
    };
    let domain: Vec<u16> = domain.iter().map(|unit| if LABEL_SEPARATORS.contains(unit) { FULL_STOP } else { *unit }).collect();
    for (index, part) in domain.split(|unit| *unit == FULL_STOP).enumerate() {
        if index > 0 {
            result.push(FULL_STOP);
        }
        result.extend(label(part)?);
    }
    Ok(result)
}

fn to_ascii_units(input: &[u16]) -> Result<Vec<u16>, Failure> {
    map_domain(input, |label| {
        if label.iter().any(|unit| u32::from(*unit) >= BASIC_LIMIT) {
            let mut encoded: Vec<u16> = ACE_PREFIX.encode_utf16().collect();
            encoded.extend(encode_units(label)?);
            Ok(encoded)
        } else {
            Ok(label.to_vec())
        }
    })
}

fn to_unicode_units(input: &[u16]) -> Result<Vec<u16>, Failure> {
    map_domain(input, |label| {
        let prefix: Vec<u16> = ACE_PREFIX.encode_utf16().collect();
        if label.starts_with(&prefix) {
            let lower: Vec<u16> = label[prefix.len()..]
                .iter()
                .map(|unit| if (0x41..=0x5a).contains(unit) { unit + 0x20 } else { *unit })
                .collect();
            Ok(ucs2_encode(&decode_points(&lower)?))
        } else {
            Ok(label.to_vec())
        }
    })
}

/// The exports' operations, in Node's order: (export, name, operation).
const FUNCTIONS: &[(&str, &str, &str)] = &[
    ("decode", "decode", "punycodeDecode"),
    ("encode", "encode", "punycodeEncode"),
    ("toASCII", "toASCII", "punycodeToASCII"),
    ("toUnicode", "toUnicode", "punycodeToUnicode"),
];

/// `node:punycode`, which warns once that it is deprecated unless required from a package.
pub(crate) fn module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let frames = c.number(DEPRECATION_FRAMES);
    let undefined = c.undefined();
    let inside = super::util_shared_vm::is_inside_node_modules(c, undefined, &[frames])?;
    if !c.truthy_rooted(inside)? {
        let process = c
            .host_mut()
            .shared_state()
            .borrow()
            .process_module
            .ok_or_else(|| RootedError::host("process is not initialized"))?;
        let emit = c.get_named_property_rooted(process, "emitWarning")?;
        let message = c.string_rooted(DEPRECATION.0);
        let kind = c.string_rooted("DeprecationWarning");
        let code = c.string_rooted(DEPRECATION.1);
        c.call_rooted(emit, process, &[message, kind, code])?;
    }
    let module = c.object_rooted()?;
    let version = c.string_rooted(VERSION);
    c.assign_named_property_rooted(module, "version", version)?;
    let ucs2 = c.object_rooted()?;
    for (name, function_name, operation_name) in [("decode", "ucs2decode", "punycodeUcs2Decode"), ("encode", "ucs2encode", "punycodeUcs2Encode")] {
        let function = c.host_function_named(operation(operation_name), function_name)?;
        c.assign_named_property_rooted(ucs2, name, function)?;
    }
    c.assign_named_property_rooted(module, "ucs2", ucs2)?;
    for &(export, name, operation_name) in FUNCTIONS {
        let function = c.host_function_named(operation(operation_name), name)?;
        c.assign_named_property_rooted(module, export, function)?;
    }
    Ok(module)
}

fn input_units(c: &mut Context<'_>, args: &[RootId]) -> Result<Vec<u16>, RootedError> {
    let undefined = c.undefined();
    let value = args.first().copied().unwrap_or(undefined);
    c.to_string_code_units(value)
}

fn finish(c: &mut Context<'_>, result: Result<Vec<u16>, Failure>) -> Result<RootId, RootedError> {
    match result {
        Ok(units) => Ok(c.string_units_rooted(&units)),
        Err(failure) => {
            let error = c.range_error_rooted(failure.message())?;
            Err(c.throw(error))
        }
    }
}

/// `punycode.decode(input)`; code points past Unicode fail as `String.fromCodePoint` does.
pub(crate) fn decode(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let input = input_units(c, args)?;
    match decode_points(&input) {
        Ok(points) => {
            if let Some(point) = points.iter().find(|point| **point > u32::from(char::MAX)) {
                let error = c.range_error_rooted(&format!("Invalid code point {point}"))?;
                return Err(c.throw(error));
            }
            Ok(c.string_units_rooted(&ucs2_encode(&points)))
        }
        Err(failure) => finish(c, Err(failure)),
    }
}

pub(crate) fn encode(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let input = input_units(c, args)?;
    finish(c, encode_units(&input))
}

pub(crate) fn to_ascii(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let input = input_units(c, args)?;
    finish(c, to_ascii_units(&input))
}

pub(crate) fn to_unicode(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let input = input_units(c, args)?;
    finish(c, to_unicode_units(&input))
}

/// `punycode.ucs2.decode(string)`.
pub(crate) fn ucs2_decode_operation(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let input = input_units(c, args)?;
    let points = ucs2_decode(&input).into_iter().map(|point| c.number(f64::from(point))).collect::<Vec<_>>();
    c.array_rooted(&points)
}

/// `punycode.ucs2.encode(codePoints)`: `String.fromCodePoint(...codePoints)`.
pub(crate) fn ucs2_encode_operation(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let points = args.first().copied().unwrap_or(undefined);
    let mut code_points = Vec::new();
    for point in c.array_like_values_rooted(points)? {
        let number = c.to_number(point)?;
        if number.fract() != 0.0 || !(0.0..=f64::from(u32::from(char::MAX))).contains(&number) {
            let text = c.to_string(point)?;
            let error = c.range_error_rooted(&format!("Invalid code point {text}"))?;
            return Err(c.throw(error));
        }
        code_points.push(number as u32);
    }
    Ok(c.string_units_rooted(&ucs2_encode(&code_points)))
}
