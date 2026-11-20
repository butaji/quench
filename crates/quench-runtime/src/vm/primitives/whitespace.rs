//! The ECMAScript WhiteSpace and LineTerminator code points `trim` and number parsing strip.

pub(super) use crate::staged::string_text::is_ecma_whitespace;

pub(in crate::vm) fn is_ecma_whitespace_character(character: char) -> bool {
    u16::try_from(u32::from(character)).is_ok_and(is_ecma_whitespace)
}
