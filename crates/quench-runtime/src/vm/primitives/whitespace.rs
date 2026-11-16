//! The ECMAScript WhiteSpace and LineTerminator code points `trim` and number parsing strip.

pub(super) fn is_ecma_whitespace(unit: u16) -> bool {
    matches!(
        unit,
        0x0009..=0x000D
            | 0x0020
            | 0x00A0
            | 0x1680
            | 0x2000..=0x200A
            | 0x2028..=0x2029
            | 0x202F
            | 0x205F
            | 0x3000
            | 0xFEFF
    )
}

pub(in crate::vm) fn is_ecma_whitespace_character(character: char) -> bool {
    u16::try_from(u32::from(character)).is_ok_and(is_ecma_whitespace)
}
