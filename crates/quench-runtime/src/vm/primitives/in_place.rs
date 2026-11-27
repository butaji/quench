//! String methods answered from a flat receiver's units in place, without copying it.

use super::*;
use crate::staged::string_replace::{self, Piece, ReplacePlan};
use crate::staged::{string_search, string_split};
use crate::vm::wtf16::Units;
use crate::vm::wtf16::split::SplitParts;

impl Vm {
    /// `indexOf`, `lastIndexOf`, `includes`, `startsWith` or `endsWith` of a flat string receiver
    /// searching for a flat string, from a position left out or passed as an integer: every
    /// conversion the method makes is then the value itself, so the search reads both strings'
    /// units in place, as `call_primitive_native` would after copying them. `None` leaves any
    /// other call to it.
    pub(super) fn string_search_in_place(&self, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
        if !matches!(
            native,
            Native::StringIndexOf
                | Native::StringLastIndexOf
                | Native::StringIncludes
                | Native::StringStartsWith
                | Native::StringEndsWith
        ) {
            return None;
        }
        let [search, rest @ ..] = args else {
            return None;
        };
        let text = self.heap.flat_text(receiver)?;
        let needle = self.heap.flat_text(*search)?;
        let (text, needle) = (text.units(), needle.units());
        let number = search_argument(native, rest.first(), text.len())?;
        let position = string_search::clamped_position(native, number, text.len());
        Some(string_search::search(native, text, needle, position))
    }

    /// `slice`, `substring` or `substr` of a flat string receiver with integer or left-out
    /// bounds: every conversion is then the value itself, so the result is cut from the units
    /// in place rather than from a copy of the receiver. `None` leaves any other call to
    /// `call_primitive_native`.
    pub(super) fn substring_in_place(&mut self, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
        if !matches!(native, Native::StringSlice | Native::StringSubstring | Native::StringSubstr) {
            return None;
        }
        let flat = self.heap.string(receiver)?.existing_flat()?;
        let length = flat.view().len() as i64;
        let range = in_place_range(native, length, args)?;
        let text = flat.substring(range);
        Some(self.heap.alloc_string(text))
    }
}

impl Vm {
    /// `split` of a flat string receiver at a flat string separator, with the limit left
    /// out or an int32: no conversion runs guest code and nothing collects while the parts are
    /// made, so the receiver is cut in place without the general path's roots and copies. The
    /// parts are gathered once and the array is made from them in one step. `None` leaves any
    /// other call to `call_primitive_native`.
    pub(in crate::vm) fn split_in_place(&mut self, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
        if native != Native::StringSplit {
            return None;
        }
        let separator = args.first()?;
        let limit = split_limit(args.get(1))?;
        let input = self.heap.string(receiver)?;
        input.existing_flat()?;
        let input = input.clone();
        let separator = self.heap.string(*separator)?;
        separator.existing_flat()?;
        let separator = separator.clone();
        let mut parts = SplitParts::<Value>::new();
        string_split::runs(input.units(), separator.units(), limit, |run| parts.push(self.split_piece(run)));
        Some(self.heap.alloc_literal_array_from_parts(self.array_proto, &[parts.as_slice()]))
    }

    /// The string of one piece of a split: an ASCII run is written as `alloc_ascii_run` makes it,
    /// inline in its record when short.
    pub(in crate::vm) fn split_piece(&mut self, run: Units<'_>) -> Value {
        match run {
            Units::Ascii(bytes) => self.heap.alloc_ascii_run(bytes),
            _ => self.heap.alloc_unit_run(run),
        }
    }
}

/// The limit a `split` takes: left out or undefined is every piece (ToUint32 of `u32::MAX`), an
/// int32 is its ToUint32. `None` for any other value, which the general path converts.
fn split_limit(limit: Option<&Value>) -> Option<usize> {
    match limit {
        Some(value) if !value.is_undefined() => value.as_int().map(|int| int as u32 as usize),
        _ => Some(u32::MAX as usize),
    }
}

impl Vm {
    /// The one body of `replace` and `replaceAll` at a string pattern: a flat string receiver,
    /// pattern and replacement (the replacement's `$` patterns are GetSubstitution with no
    /// captures, which `replacement_substitution` makes). No conversion here runs guest code, so
    /// the result is written once, the unmatched runs copied around each replacement. A
    /// replacement function, a non-string pattern or a rope leaves the call to the general path
    /// (`string_replace_native`), which answers `None` here.
    pub(in crate::vm) fn replace_string_pattern(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        receiver: Value,
        args: &[Value],
    ) -> Option<Result<Value, JsError>> {
        if !matches!(native, Native::StringReplace | Native::StringReplaceAll) {
            return None;
        }
        let [search, replacement, ..] = args else {
            return None;
        };
        let replace_all = native == Native::StringReplaceAll;
        if let Some(replaced) = self.ascii_replace(receiver, *search, *replacement, replace_all) {
            return Some(Ok(replaced));
        }
        let input = self.flat_string(receiver)?;
        let search = self.flat_string(*search)?;
        let template = self.flat_string(*replacement)?;
        Some(self.replace_units(p, &input, &search, &template, replace_all).map(|replaced| replaced.unwrap_or(receiver)))
    }

    /// `replace` or `replaceAll` of a flat ASCII receiver at a flat ASCII pattern, with a flat
    /// ASCII replacement that holds no `$`. The three operands are read in place once: the
    /// matches are found and the result is written from the same borrow, then allocated after
    /// the borrow ends. The receiver itself when nothing matches. `None` leaves the call to the
    /// general path.
    fn ascii_replace(&mut self, receiver: Value, search: Value, replacement: Value, replace_all: bool) -> Option<Value> {
        let plan = self
            .heap
            .with_ascii_sources([receiver, search, replacement], |[text, needle, with]| string_replace::ascii_plan(text, needle, with, replace_all))
            .flatten()?;
        match plan {
            ReplacePlan::Unchanged => Some(receiver),
            ReplacePlan::Derived(string) => Some(self.heap.alloc_string(string)),
        }
    }

    /// A string value's flat units, read in place: a rope is left to the general path, which
    /// flattens it.
    fn flat_string(&self, value: Value) -> Option<JsString> {
        let string = self.heap.string(value)?;
        string.existing_flat()?;
        Some(string.clone())
    }

    /// The replacement text of one match at `index`: `template` itself, or its substitution
    /// (`$$`, `$&`, `` $` ``, `$'`) where it holds a `$`, appended to `output`.
    #[allow(clippy::too_many_arguments)]
    fn append_replacement(
        &mut self,
        p: &ResidualProgram,
        output: &mut crate::vm::wtf16::JsStringBuilder,
        substitutes: bool,
        template: &JsString,
        input: &JsString,
        index: usize,
        search: &JsString,
    ) -> Result<(), JsError> {
        if substitutes {
            let text = self.replacement_substitution(p, template, input, index, search, &[], Value::UNDEFINED)?;
            output.append(&text);
        } else {
            output.append(template);
        }
        Ok(())
    }

    /// The general write of `replace_string_pattern`, of units of either width: the matches of
    /// `string_replace::match_starts`, each taking the replacement template or its GetSubstitution
    /// when the template has a `$`. `None` when nothing matches, so the caller keeps its receiver.
    pub(in crate::vm) fn replace_units(
        &mut self,
        p: &ResidualProgram,
        input: &JsString,
        search: &JsString,
        template: &JsString,
        replace_all: bool,
    ) -> Result<Option<Value>, JsError> {
        let mut starts = string_replace::match_starts(input.units(), search.units(), replace_all);
        let Some(first) = starts.next() else {
            return Ok(None);
        };
        let units = template.units();
        let substitutes = (0..units.len()).any(|index| units.at(index) == u16::from(b'$'));
        let search_length = search.units().len();
        let mut output = crate::vm::wtf16::JsStringBuilder::default();
        string_replace::walk_matches(
            input.units().len(),
            std::iter::once(first).chain(starts),
            |&index| index..index + search_length,
            |piece| -> Result<(), JsError> {
                match piece {
                    Piece::Gap(gap) => {
                        output.append_slice(input, gap);
                        Ok(())
                    }
                    Piece::Match(index) => self.append_replacement(p, &mut output, substitutes, template, input, index, search),
                }
            },
        )?;
        let output = self.string_build_result(p, output.finish())?;
        Ok(Some(self.heap.alloc_string(output)))
    }
}

/// The units range a `slice`, `substring` or `substr` cuts from a flat string of `length`
/// units for its integer arguments, where a left-out or undefined argument takes the method's
/// default. `None` for any other argument, which the general path converts.
fn in_place_range(native: Native, length: i64, args: &[Value]) -> Option<std::ops::Range<usize>> {
    let bound = |index: usize, default: i64| match args.get(index) {
        Some(value) if !value.is_undefined() => value.as_int().map(i64::from),
        _ => Some(default),
    };
    let end_default = if native == Native::StringSubstr { string_search::SUBSTR_REST } else { length };
    Some(string_search::cut(native, length, bound(0, 0)?, bound(1, end_default)?))
}

/// A search's position argument as the number the in-place body searches from: an integer
/// argument as its value, a left-out or undefined one as `search_default`. `None` for any other
/// value, which the general path converts.
fn search_argument(native: Native, argument: Option<&Value>, length: usize) -> Option<f64> {
    match argument {
        Some(value) if !value.is_undefined() => value.as_int().map(f64::from),
        _ => Some(string_search::search_default(native, length)),
    }
}
