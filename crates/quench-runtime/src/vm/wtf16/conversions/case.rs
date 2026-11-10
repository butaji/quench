//! The default (locale-free) full case mapping of `String.prototype.toUpperCase` and
//! `toLowerCase`, written straight into the result's units.

use super::super::{FlatUnits, JsString, Units};
use std::borrow::Cow;

/// The one letter whose default lowercase depends on its context (SpecialCasing's
/// Final_Sigma): its strings take the run-wise host mapping (`vm::string::map_case`).
const CAPITAL_SIGMA: char = '\u{03A3}';

impl JsString {
    /// This string's full case mapping, `upper` or lower: the string itself where no unit
    /// changes, as primitive strings have no identity; `None` where a capital sigma's
    /// lowercase needs its context.
    pub(crate) fn default_case(&self, upper: bool) -> Option<Cow<'_, JsString>> {
        let mapped = match self.units() {
            Units::Ascii(bytes) => ascii_case(bytes, upper),
            Units::Wide(units) => wide_case(units, upper)?,
        };
        Some(mapped.map_or(Cow::Borrowed(self), Cow::Owned))
    }
}

fn ascii_byte_case(byte: u8, upper: bool) -> u8 {
    if upper { byte.to_ascii_uppercase() } else { byte.to_ascii_lowercase() }
}

/// ASCII maps to ASCII unit for unit, straight into the result's storage; `None` where
/// nothing changes.
fn ascii_case(bytes: &[u8], upper: bool) -> Option<JsString> {
    bytes.iter().position(|byte| ascii_byte_case(*byte, upper) != *byte)?;
    Some(JsString::from_flat(FlatUnits::ascii_with(bytes.len(), |output| {
        for (slot, byte) in output.iter_mut().zip(bytes) {
            slot.write(ascii_byte_case(*byte, upper));
        }
    })))
}

/// Each character's full mapping in order, its units unchanged up to the first that maps
/// elsewhere; a lone surrogate is not cased and maps to itself. `Some(None)` where nothing
/// changes, `None` where a capital sigma lowers.
fn wide_case(units: &[u16], upper: bool) -> Option<Option<JsString>> {
    let Some(first) = units.iter().position(|unit| !maps_to_itself(*unit, upper)) else {
        return Some(None);
    };
    let mut output = Vec::with_capacity(units.len());
    output.extend_from_slice(&units[..first]);
    let mut index = first;
    while let Some(&unit) = units.get(index) {
        index += 1;
        if let Some(byte) = ascii_unit(unit) {
            output.push(u16::from(ascii_byte_case(byte, upper)));
            continue;
        }
        let pair = units.get(index).and_then(|low| crate::unicode::decode_surrogate_pair(unit, *low));
        index += usize::from(pair.is_some());
        match char::from_u32(pair.unwrap_or_else(|| u32::from(unit))) {
            Some(CAPITAL_SIGMA) if !upper => return None,
            Some(character) => push_case(&mut output, character, upper),
            None => output.push(unit),
        }
    }
    Some(Some(JsString::from_units(&output)))
}

fn ascii_unit(unit: u16) -> Option<u8> {
    u8::try_from(unit).ok().filter(u8::is_ascii)
}

/// Whether `unit` is a whole character whose mapping is itself.
fn maps_to_itself(unit: u16, upper: bool) -> bool {
    if let Some(byte) = ascii_unit(unit) {
        return ascii_byte_case(byte, upper) == byte;
    }
    let Some(character) = char::from_u32(u32::from(unit)) else { return false };
    if upper { is_only(character, character.to_uppercase()) } else { is_only(character, character.to_lowercase()) }
}

fn is_only(character: char, mut mapped: impl ExactSizeIterator<Item = char>) -> bool {
    mapped.len() == 1 && mapped.next() == Some(character)
}

/// The most UTF-16 units one character takes: a surrogate pair.
const CHARACTER_MAX_UNITS: usize = 2;

/// `character`'s full mapping, as UTF-16 units onto `output`.
fn push_case(output: &mut Vec<u16>, character: char, upper: bool) {
    let mut buffer = [0; CHARACTER_MAX_UNITS];
    let mut push = |mapped: char| output.extend_from_slice(mapped.encode_utf16(&mut buffer));
    if upper { character.to_uppercase().for_each(&mut push) } else { character.to_lowercase().for_each(&mut push) }
}

#[cfg(test)]
mod tests;
