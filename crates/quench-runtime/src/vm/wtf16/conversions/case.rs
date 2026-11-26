//! The default (locale-free) full case mapping of `String.prototype.toUpperCase` and
//! `toLowerCase`, written straight into the result's units.

use super::super::{FlatUnits, JsString, Units};
use crate::staged::string_text;
use std::borrow::Cow;
use std::rc::Rc;

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
            Units::Latin1(bytes) => latin1_case(bytes, upper)?,
            Units::Wide(units) => wide_case(units, upper)?,
        };
        Some(mapped.map_or(Cow::Borrowed(self), Cow::Owned))
    }
}

/// ASCII maps to ASCII unit for unit, straight into the result's storage; `None` where
/// nothing changes.
fn ascii_case(bytes: &[u8], upper: bool) -> Option<JsString> {
    if !string_text::ascii_case_changes(bytes, upper) {
        return None;
    }
    Some(JsString::from_flat(FlatUnits::ascii_with(bytes.len(), |output| {
        // The case map writes every byte of the output (`Stage::case_span`).
        string_text::ascii_case_raw(bytes, upper, output.as_mut_ptr().cast());
    })))
}

/// Latin-1 text maps unit for unit into Latin-1 storage, written straight into the result. A
/// character whose full mapping is not one Latin-1 character (U+00B5, U+00DF and U+00FF
/// uppercased) makes the whole string take the wide mapping instead. Non-ASCII Latin-1 maps to
/// non-ASCII Latin-1, so the result is already in canonical storage. `Some(None)` where nothing
/// changes.
fn latin1_case(bytes: &[u8], upper: bool) -> Option<Option<JsString>> {
    // ASCII's mapping is the same unit for unit, so ASCII-only text takes its fast path.
    if bytes.is_ascii() {
        return Some(ascii_case(bytes, upper));
    }
    let Some(first) = bytes.iter().position(|byte| latin1_case_byte(*byte, upper) != Some(*byte)) else {
        return Some(None);
    };
    let mut output = Vec::with_capacity(bytes.len());
    output.extend_from_slice(&bytes[..first]);
    for byte in &bytes[first..] {
        let Some(mapped) = latin1_case_byte(*byte, upper) else {
            return wide_case(&bytes.iter().map(|unit| u16::from(*unit)).collect::<Vec<u16>>(), upper);
        };
        output.push(mapped);
    }
    let storage: Rc<[u8]> = Rc::from(output);
    Some(Some(JsString::from_flat(FlatUnits::Latin1(storage))))
}

/// The Latin-1 unit that `byte`'s full mapping is; `None` where that mapping is not one
/// Latin-1 character.
fn latin1_case_byte(byte: u8, upper: bool) -> Option<u8> {
    if byte.is_ascii() {
        return Some(string_text::ascii_case_byte(byte, upper));
    }
    let character = char::from(byte);
    let mut upper_map;
    let mut lower_map;
    let mapped: &mut dyn Iterator<Item = char> = if upper {
        upper_map = character.to_uppercase();
        &mut upper_map
    } else {
        lower_map = character.to_lowercase();
        &mut lower_map
    };
    let only = mapped.next()?;
    if mapped.next().is_some() {
        return None;
    }
    u8::try_from(u32::from(only)).ok()
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
            output.push(u16::from(string_text::ascii_case_byte(byte, upper)));
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
        return string_text::ascii_case_byte(byte, upper) == byte;
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
