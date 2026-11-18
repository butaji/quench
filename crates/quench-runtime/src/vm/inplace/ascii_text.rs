//! In-place ASCII fast paths for String builtins: each reads a flat ASCII receiver once, completes
//! exactly as the spec says with no guest code and no error, or answers `None` so the general
//! native runs.
use super::super::*;
use super::super::wtf16::Units;
use smallvec::SmallVec;

/// The longest result a fast path builds; past it the general native decides.
const FAST_TEXT_MAX_BYTES: usize = 1 << 20;
/// Result bytes held on the stack before their one copy into the string.
const FAST_TEXT_INLINE_BYTES: usize = 64;
/// ECMAScript WhiteSpace and LineTerminator restricted to ASCII.
const ASCII_WHITESPACE: [u8; 6] = [b'\t', b'\n', b'\x0B', b'\x0C', b'\r', b' '];
/// The filler of `padStart` and `padEnd` when none is given.
const DEFAULT_PAD: &[u8] = b" ";

/// What a fast path decided: the receiver itself, or a new string to allocate.
enum AsciiOutcome {
    Unchanged,
    New(SmallVec<[u8; FAST_TEXT_INLINE_BYTES]>),
}

impl<H: Host> Vm<H> {
    /// The String builtin `native` on `receiver` with `args`, when it completes in place over a
    /// flat ASCII receiver; `None` for any other receiver, argument or result size.
    pub(in crate::vm) fn ascii_text_method(&mut self, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
        match self.ascii_text_outcome(native, receiver, args)? {
            AsciiOutcome::Unchanged => Some(receiver),
            AsciiOutcome::New(bytes) => Some(self.heap.alloc_ascii_run(&bytes)),
        }
    }

    /// The outcome of `native` over the receiver's flat ASCII bytes, read once.
    fn ascii_text_outcome(&self, native: Native, receiver: Value, args: &[Value]) -> Option<AsciiOutcome> {
        let text = self.heap.flat_text(receiver)?;
        let Units::Ascii(bytes) = text.units() else {
            return None;
        };
        match native {
            Native::StringToUpperCase => Some(ascii_case(bytes, u8::is_ascii_lowercase, u8::to_ascii_uppercase)),
            Native::StringToLowerCase => Some(ascii_case(bytes, u8::is_ascii_uppercase, u8::to_ascii_lowercase)),
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
        if target <= text.len() {
            return Some(AsciiOutcome::Unchanged);
        }
        match args.get(1).copied() {
            None | Some(Value::UNDEFINED) => pad_with(native, text, target, DEFAULT_PAD),
            Some(value) => {
                let fill = self.heap.flat_text(value)?;
                let Units::Ascii(fill_bytes) = fill.units() else {
                    return None;
                };
                pad_with(native, text, target, fill_bytes)
            }
        }
    }
}

/// The ASCII case map of `bytes`, where `needs_change` names the bytes it changes; the receiver
/// itself when none does.
fn ascii_case(bytes: &[u8], needs_change: fn(&u8) -> bool, map: fn(&u8) -> u8) -> AsciiOutcome {
    if !bytes.iter().any(needs_change) {
        return AsciiOutcome::Unchanged;
    }
    let mapped: SmallVec<[u8; FAST_TEXT_INLINE_BYTES]> = bytes.iter().map(map).collect();
    AsciiOutcome::New(mapped)
}

/// `trim`, `trimStart` or `trimEnd` of ASCII `bytes`: the whitespace stripped from the chosen
/// ends; the receiver itself when nothing is stripped.
fn ascii_trim(bytes: &[u8], from_start: bool, from_end: bool) -> AsciiOutcome {
    let is_space = |byte: &u8| ASCII_WHITESPACE.contains(byte);
    let start = if from_start {
        bytes.iter().position(|byte| !is_space(byte)).unwrap_or(bytes.len())
    } else {
        0
    };
    let end = if from_end {
        bytes.iter().rposition(|byte| !is_space(byte)).map_or(start, |last| last + 1)
    } else {
        bytes.len()
    };
    if start == 0 && end == bytes.len() {
        return AsciiOutcome::Unchanged;
    }
    AsciiOutcome::New(SmallVec::from_slice(&bytes[start..end]))
}

/// `text` padded to `target` units with repeats of `fill`, in place at the end or the start.
fn pad_with(native: Native, text: &[u8], target: usize, fill: &[u8]) -> Option<AsciiOutcome> {
    if fill.is_empty() {
        return Some(AsciiOutcome::Unchanged);
    }
    if target > FAST_TEXT_MAX_BYTES {
        return None;
    }
    let padding = fill.iter().copied().cycle().take(target - text.len());
    let mut bytes: SmallVec<[u8; FAST_TEXT_INLINE_BYTES]> = SmallVec::with_capacity(target);
    if native == Native::StringPadEnd {
        bytes.extend_from_slice(text);
        bytes.extend(padding);
    } else {
        bytes.extend(padding);
        bytes.extend_from_slice(text);
    }
    Some(AsciiOutcome::New(bytes))
}

/// `repeat` of ASCII `bytes` by the non-negative int32 count in `args`; `None` for a negative
/// count (a RangeError) or a result past `FAST_TEXT_MAX_BYTES`.
fn ascii_repeat(bytes: &[u8], args: &[Value]) -> Option<AsciiOutcome> {
    let count = usize::try_from(args.first()?.as_int()?).ok()?;
    if bytes.is_empty() || count == 1 {
        return Some(AsciiOutcome::Unchanged);
    }
    if count == 0 {
        return Some(AsciiOutcome::New(SmallVec::new()));
    }
    let size = bytes.len().checked_mul(count)?;
    if size > FAST_TEXT_MAX_BYTES {
        return None;
    }
    let repeated: SmallVec<[u8; FAST_TEXT_INLINE_BYTES]> = bytes.iter().copied().cycle().take(size).collect();
    Some(AsciiOutcome::New(repeated))
}
