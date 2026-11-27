#![allow(unused_imports)]
use super::*;
use crate::staged::Stage;
use crate::staged::region::Region;

/// The units of a span on `Concrete`, as the tuned searches take them.
#[cfg(not(stencil_build))]
pub fn concrete_units<'a>(span: UnitSpan<u64>) -> crate::vm::wtf16::Units<'a> {
    use crate::vm::wtf16::Units;
    let length = span.length as usize;
    // SAFETY: a Concrete span names the live units of a string the caller holds for the call.
    unsafe {
        match span.form {
            SpanForm::Ascii => Units::Ascii(std::slice::from_raw_parts(span.units as *const u8, length)),
            SpanForm::Latin1 => Units::Latin1(std::slice::from_raw_parts(span.units as *const u8, length)),
            SpanForm::Wide => Units::Wide(std::slice::from_raw_parts(span.units as *const u16, length)),
        }
    }
}

/// The span of `units` on `Concrete`: its form, address and length.
#[cfg(not(stencil_build))]
pub fn concrete_span(units: crate::vm::wtf16::Units<'_>) -> UnitSpan<u64> {
    use crate::vm::wtf16::Units;
    match units {
        Units::Ascii(bytes) => UnitSpan { form: SpanForm::Ascii, units: bytes.as_ptr() as u64, length: bytes.len() as u64 },
        Units::Latin1(bytes) => UnitSpan { form: SpanForm::Latin1, units: bytes.as_ptr() as u64, length: bytes.len() as u64 },
        Units::Wide(wide) => UnitSpan { form: SpanForm::Wide, units: wide.as_ptr() as u64, length: wide.len() as u64 },
    }
}
