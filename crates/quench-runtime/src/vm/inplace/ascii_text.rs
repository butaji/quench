//! In-place ASCII fast paths for String builtins: each reads a flat ASCII receiver once, completes
//! exactly as the spec says with no guest code and no error, or answers `None` so the general
//! native runs. A result is written once, into its string record. The text rules are
//! `staged::string_text`'s, which the general natives share.
use super::super::*;
use super::super::wtf16::{JsString, Units};
use crate::staged::string_text;
use std::ops::Range;

/// The longest result a fast path builds; past it the general native decides.
const FAST_TEXT_MAX_BYTES: usize = 1 << 20;

/// What a fast path decided, from the receiver's bytes read in place.
enum AsciiOutcome {
    /// The receiver itself.
    Unchanged,
    /// The receiver's bytes in `range`, as a new string.
    Slice(Range<usize>),
    /// The receiver's bytes case-mapped (upper or lower), as a new string of `length` bytes.
    Cased { length: usize, upper: bool },
    /// The receiver padded to `length` bytes with repeats of the filler `fill` (a string
    /// value; `None` for a space), at its end or its start.
    Padded { length: usize, fill: Option<Value>, at_end: bool },
    /// The receiver repeated to `length` bytes.
    Repeated { length: usize },
}

impl<H: Host> Vm<H> {
    /// The String builtin `native` on `receiver` with `args`, when it completes in place over a
    /// flat ASCII receiver; `None` for any other receiver, argument or result size.
    pub(in crate::vm) fn ascii_text_method(&mut self, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
        let outcome = self.ascii_text_outcome(native, receiver, args)?;
        // Each result is written once, from the receiver's bytes (and the filler's) read in place.
        let heap = &mut self.heap;
        match outcome {
            AsciiOutcome::Unchanged => Some(receiver),
            AsciiOutcome::Slice(range) => {
                heap.alloc_ascii_derived([receiver], range.len(), move |out, [text]| out.copy_from_slice(&text[range]))
            }
            AsciiOutcome::Cased { length, upper } => {
                heap.alloc_ascii_derived([receiver], length, move |out, [text]| string_text::ascii_case(text, upper, out))
            }
            AsciiOutcome::Padded { length, fill: None, at_end } => heap.alloc_ascii_derived([receiver], length, move |out, [text]| {
                string_text::pad_into(out, text, string_text::PAD_FILL, at_end)
            }),
            AsciiOutcome::Padded { length, fill: Some(fill), at_end } => {
                heap.alloc_ascii_derived([receiver, fill], length, move |out, [text, fill]| {
                    string_text::pad_into(out, text, fill, at_end)
                })
            }
            AsciiOutcome::Repeated { length } => {
                heap.alloc_ascii_derived([receiver], length, |out, [text]| string_text::repeat_into(out, text))
            }
        }
    }

    /// The outcome of `native` over the receiver's flat ASCII bytes, read in place.
    fn ascii_text_outcome(&self, native: Native, receiver: Value, args: &[Value]) -> Option<AsciiOutcome> {
        let text = self.heap.flat_text(receiver)?;
        let Units::Ascii(bytes) = text.units() else {
            return None;
        };
        match native {
            Native::StringToUpperCase => Some(ascii_case(bytes, true)),
            Native::StringToLowerCase => Some(ascii_case(bytes, false)),
            Native::StringTrim => Some(ascii_trim(bytes, true, true)),
            Native::StringTrimStart => Some(ascii_trim(bytes, true, false)),
            Native::StringTrimEnd => Some(ascii_trim(bytes, false, true)),
            Native::StringPadStart | Native::StringPadEnd => self.ascii_pad(native, bytes, args),
            Native::StringRepeat => ascii_repeat(bytes, args),
            _ => None,
        }
    }

    /// `padStart` or `padEnd` of ASCII `text` with an ASCII filler (default a space).
    fn ascii_pad(&self, native: Native, text: &[u8], args: &[Value]) -> Option<AsciiOutcome> {
        let max_length = i64::from(args.first()?.as_int()?);
        // A negative maxLength is ToLength 0, which never pads.
        let Ok(target) = usize::try_from(max_length) else {
            return Some(AsciiOutcome::Unchanged);
        };
        let Some(length) = string_text::pad_length(text.len(), target) else {
            return Some(AsciiOutcome::Unchanged);
        };
        let fill = match args.get(1).copied() {
            None | Some(Value::UNDEFINED) => None,
            Some(value) => Some(value),
        };
        // An empty filler never pads; a filler that is not flat ASCII is the general path's.
        let fill_len = match fill {
            None => string_text::PAD_FILL.len(),
            Some(value) => self.heap.existing_ascii_len(value)?,
        };
        if fill_len == 0 {
            return Some(AsciiOutcome::Unchanged);
        }
        if length > FAST_TEXT_MAX_BYTES {
            return None;
        }
        Some(AsciiOutcome::Padded { length, fill, at_end: native == Native::StringPadEnd })
    }
}

/// The flat ASCII text of `value` as a string owned apart from the heap, so a result can be
/// written into the heap while its text is read: a clone of the record (sharing its storage),
/// or of a small string's text. `None` for a wide or non-flat string.
pub(super) fn owned_flat_ascii(heap: &crate::heap::Heap, value: Value) -> Option<JsString> {
    let text = match heap.flat_text(value)? {
        crate::heap::FlatText::Record(Units::Ascii(_)) => heap.string(value)?.clone(),
        crate::heap::FlatText::Record(Units::Wide(_) | Units::Latin1(_)) => return None,
        crate::heap::FlatText::Small(text) => JsString::from_shared_text(&text),
    };
    let ascii = matches!(text.units(), Units::Ascii(_));
    ascii.then_some(text)
}

/// The bytes of `string` when it is ASCII.
pub(super) fn ascii_bytes(string: &JsString) -> Option<&[u8]> {
    match string.units() {
        Units::Ascii(bytes) => Some(bytes),
        Units::Latin1(_) | Units::Wide(_) => None,
    }
}

/// The ASCII case map of `bytes`, upper or lower; the receiver itself when no byte changes.
fn ascii_case(bytes: &[u8], upper: bool) -> AsciiOutcome {
    if string_text::ascii_case_changes(bytes, upper) {
        AsciiOutcome::Cased { length: bytes.len(), upper }
    } else {
        AsciiOutcome::Unchanged
    }
}

/// `trim`, `trimStart` or `trimEnd` of ASCII `bytes`: the whitespace stripped from the chosen
/// ends; the receiver itself when nothing is stripped.
fn ascii_trim(bytes: &[u8], from_start: bool, from_end: bool) -> AsciiOutcome {
    let range = string_text::trim_range(bytes, from_start, from_end);
    if range.len() == bytes.len() {
        return AsciiOutcome::Unchanged;
    }
    AsciiOutcome::Slice(range)
}

/// `repeat` of ASCII `bytes` by the non-negative int32 count in `args`; `None` for a negative
/// count (a RangeError) or a result past `FAST_TEXT_MAX_BYTES`.
fn ascii_repeat(bytes: &[u8], args: &[Value]) -> Option<AsciiOutcome> {
    let count = usize::try_from(args.first()?.as_int()?).ok()?;
    if bytes.is_empty() || count == 1 {
        return Some(AsciiOutcome::Unchanged);
    }
    if count == 0 {
        return Some(AsciiOutcome::Slice(0..0));
    }
    let length = bytes.len().checked_mul(count)?;
    if length > FAST_TEXT_MAX_BYTES {
        return None;
    }
    Some(AsciiOutcome::Repeated { length })
}
