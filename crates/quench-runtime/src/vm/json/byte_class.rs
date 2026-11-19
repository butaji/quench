//! The byte classes the JSON parser dispatches on.

/// The classes of the bytes the parser dispatches on: whitespace, the starts of a string, a
/// number, a container or a literal, and every other byte.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum JsonByteClass {
    /// A byte no token starts with.
    Other,
    /// JSON whitespace: space, tab, line feed and carriage return.
    Whitespace,
    /// A quote, which starts a string.
    Quote,
    /// A minus sign, which starts a number.
    Minus,
    /// A decimal digit, which starts a number.
    Digit,
    /// A left brace, which starts an object.
    ObjectStart,
    /// A left bracket, which starts an array.
    ArrayStart,
    /// The `n` of `null`.
    Null,
    /// The `t` of `true`.
    True,
    /// The `f` of `false`.
    False,
}

/// The bytes JSON whitespace is made of: space, tab, line feed and carriage return.
pub(super) const WHITESPACE_BYTES: [u8; 4] = [b' ', b'\t', b'\n', b'\r'];

/// The class of every byte, so the parser learns a byte's class from one load of this table
/// (`byte_class_of`).
const JSON_BYTE_CLASSES: [JsonByteClass; 1 << u8::BITS] = {
    let mut classes = [JsonByteClass::Other; 1 << u8::BITS];
    let mut whitespace = 0;
    while whitespace < WHITESPACE_BYTES.len() {
        classes[WHITESPACE_BYTES[whitespace] as usize] = JsonByteClass::Whitespace;
        whitespace += 1;
    }
    classes[b'"' as usize] = JsonByteClass::Quote;
    classes[b'-' as usize] = JsonByteClass::Minus;
    let mut digit = b'0';
    while digit <= b'9' {
        classes[digit as usize] = JsonByteClass::Digit;
        digit += 1;
    }
    classes[b'{' as usize] = JsonByteClass::ObjectStart;
    classes[b'[' as usize] = JsonByteClass::ArrayStart;
    classes[b'n' as usize] = JsonByteClass::Null;
    classes[b't' as usize] = JsonByteClass::True;
    classes[b'f' as usize] = JsonByteClass::False;
    classes
};

/// The class of `byte`. The table holds every byte, so the lookup needs no bounds check.
#[inline(always)]
pub(super) fn byte_class_of(byte: u8) -> JsonByteClass {
    JSON_BYTE_CLASSES[usize::from(byte)]
}

/// The class of `unit`, which is `Other` past the bytes the table holds.
#[inline(always)]
pub(super) fn byte_class(unit: u16) -> JsonByteClass {
    match u8::try_from(unit) {
        Ok(byte) => byte_class_of(byte),
        Err(_) => JsonByteClass::Other,
    }
}

/// Whether `unit` is an exponent mark, `e` or `E`, which may follow a number's digits.
#[inline(always)]
pub(super) fn exponent_mark(unit: Option<u16>) -> bool {
    unit.is_some_and(|unit| unit == b'e' as u16 || unit == b'E' as u16)
}
