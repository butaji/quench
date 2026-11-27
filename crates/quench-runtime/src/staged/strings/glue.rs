#![allow(unused_imports)]
use super::*;
use crate::staged::builtins::{UnitWidth, char_at_ascii, char_at_ascii_within, char_code_at, char_code_at_within};
use crate::staged::region::Region;
use crate::staged::Stage;
use crate::Value;
use crate::vm::wtf16::{JsString, Units};

#[cfg(not(stencil_build))]
/// A flat string a concrete caller holds, in either form: its record among the string records
/// at `records`, read in place, or its units already viewed, which covers the forms a record
/// read leaves.
#[derive(Clone, Copy)]
pub enum ConcreteString<'a> {
    /// The string `value` among the string records at `records`.
    Record { records: *const JsString, value: Value },
    /// The units of a flat string.
    Units(Units<'a>),
}

#[cfg(not(stencil_build))]
impl ConcreteString<'_> {
    /// This string's units as the words a row reads, or `None` where a record read leaves.
    #[inline(always)]
    fn flat(self) -> Option<FlatString<u64>> {
        match self {
            Self::Record { records, value } => {
                let mut stage = crate::staged::Concrete::pure();
                flat_string(&mut stage, records as u64, value.to_bits(), ())
            }
            Self::Units(units) => {
                let wide = u64::from(matches!(units, Units::Wide(_)));
                let (base, length) = match units {
                    Units::Ascii(bytes) => (bytes.as_ptr() as u64, bytes.len() as u64),
                    Units::Wide(units) => (units.as_ptr() as u64, units.len() as u64),
                    // Latin-1 is no native row's form: the row leaves at its miss.
                    Units::Latin1(_) => return None,
                };
                Some(FlatString { units: base, length, wide })
            }
        }
    }
}

#[cfg(not(stencil_build))]
/// The interpreter's `string_char_code_at` of `string` at `index`; `None` where the row leaves,
/// which the caller answers through the native.
#[inline(always)]
pub fn concrete_char_code_at(string: ConcreteString<'_>, index: Value) -> Option<Value> {
    let string = string.flat()?;
    let mut stage = crate::staged::Concrete::pure();
    step_string_native_of(&mut stage, StringNative::CharCodeAt { string, index: index.to_bits() }, ()).map(Value::from_bits)
}

#[cfg(not(stencil_build))]
/// The interpreter's `string_char_at` of `string` at `index`, from the table of one-unit strings
/// at `table`; `None` where the row leaves.
#[inline(always)]
pub fn concrete_char_at(string: ConcreteString<'_>, table: *const Value, index: Value) -> Option<Value> {
    let string = string.flat()?;
    let mut stage = crate::staged::Concrete::pure();
    step_string_native_of(&mut stage, StringNative::CharAt { string, index: index.to_bits(), table: table as u64 }, ()).map(Value::from_bits)
}

/// The unit index that `String.prototype.at` (`from_end`) or `codePointAt` reads for the relative
/// `index` of `units` (`relative_index_row`); `None` where it lies outside.
#[cfg(not(stencil_build))]
pub fn relative_unit(units: Units<'_>, index: i64, from_end: bool) -> Option<usize> {
    relative_index(index, units.len(), from_end)
}

/// `code_point_at_row` on `Concrete` over `units`; `None` past the end.
#[cfg(not(stencil_build))]
pub fn code_point_at(units: Units<'_>, index: usize) -> Option<u32> {
    if index >= units.len() {
        return None;
    }
    let span = crate::staged::spans::concrete_span(units);
    let mut stage = crate::staged::Concrete::pure();
    Some(code_point_at_row(&mut stage, span.form.wide(), span.units, span.length, index as u64) as u32)
}
