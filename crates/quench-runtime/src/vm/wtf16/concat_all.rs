//! Strings joined from many parts: a transient plan of slices (`JsStringBuilder`), and a
//! template literal's parts.
use super::*;

/// Parts a builder holds before it allocates: a replacement's prefix, substitution and
/// rest, or a short join, need no allocation beside the result.
const BUILDER_INLINE_PARTS: usize = 4;

struct StringPart {
    units: FlatUnits,
    range: Range<usize>,
}

/// A transient concatenation plan, not a guest rope. Slices retain only the
/// canonical storage. Overflow stops storage growth, but callers still
/// perform all replacement effects before finish reports the error.
pub(in crate::vm) struct JsStringBuilder {
    parts: smallvec::SmallVec<[StringPart; BUILDER_INLINE_PARTS]>,
    length: Result<usize, StringBuildError>,
}

impl Default for JsStringBuilder {
    fn default() -> Self {
        Self {
            parts: smallvec::SmallVec::new(),
            length: Ok(0),
        }
    }
}

impl JsStringBuilder {
    pub(in crate::vm) fn append(&mut self, string: &JsString) {
        let units = string.flat();
        let length = units.view().len();
        self.append_units(units.clone(), 0..length);
    }

    pub(in crate::vm) fn append_slice(&mut self, string: &JsString, range: Range<usize>) {
        self.append_units(string.flat().clone(), range);
    }

    fn append_units(&mut self, units: FlatUnits, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        let Ok(current) = self.length else {
            return;
        };
        self.length = checked_string_length(current, range.len());
        if self.length.is_err() {
            self.parts.clear();
            return;
        }
        if let Some(previous) = self.parts.last_mut()
            && previous.units.same_storage(&units) && previous.range.end == range.start {
                previous.range.end = range.end;
                return;
            }
        if self.parts.try_reserve(1).is_err() {
            self.length = Err(StringBuildError::Allocation);
            self.parts.clear();
            return;
        }
        self.parts.push(StringPart { units, range });
    }

    pub(in crate::vm) fn finish(self) -> Result<JsString, StringBuildError> {
        let length = self.length?;
        if self.parts.iter().all(|part| part.units.is_ascii()) {
            let bytes = self.parts.iter().map(|part| match part.units.view().slice(part.range.clone()) {
                Units::Ascii(bytes) => bytes,
                Units::Wide(_) => unreachable!("every part is ASCII"),
            });
            return Ok(JsString::from_ascii_parts(length, bytes));
        }
        let mut storage = Rc::<[u16]>::new_uninit_slice(length);
        let output = Rc::get_mut(&mut storage).expect("a new allocation is unshared");
        let mut written = 0;
        for part in &self.parts {
            let units = part.units.view().slice(part.range.clone());
            let target = &mut output[written..written + units.len()];
            match units {
                Units::Ascii(bytes) => {
                    for (unit, byte) in target.iter_mut().zip(bytes) {
                        unit.write(u16::from(*byte));
                    }
                }
                Units::Wide(units) => {
                    target.write_copy_of_slice(units);
                }
            }
            written += units.len();
        }
        assert_eq!(written, length, "the parts fill the string's length");
        // SAFETY: the parts wrote every unit.
        let storage = unsafe { storage.assume_init() };
        // Wide parts can contribute only ASCII units; such text is stored narrow.
        if storage.iter().all(|unit| is_ascii_unit(*unit)) {
            return Ok(JsString::from_units(&storage));
        }
        Ok(JsString::from_flat(FlatUnits::Wide(storage)))
    }
}


impl JsString {
    /// `parts` in order as one string: copied once into storage of their total length while
    /// that is short, else joined one after another as `concat` joins two, which keeps long
    /// text shared rather than copied.
    pub(in crate::vm) fn concat_all(parts: &[Self]) -> Result<Self, StringBuildError> {
        let length = parts.iter().try_fold(0, |length, part| checked_string_length(length, part.len()))?;
        if !Self::copies_whole(length) {
            let mut joined = Self::from_str("");
            for part in parts {
                joined.push_js_string(part);
            }
            return Ok(joined);
        }
        let mut builder = JsStringBuilder::default();
        for part in parts {
            builder.append(part);
        }
        builder.finish()
    }

    /// ASCII `parts` in order as one string of their total `length`, written straight into
    /// its storage: inline when short, else one allocation.
    pub(in crate::vm) fn from_ascii_parts<'a>(length: usize, parts: impl Iterator<Item = &'a [u8]>) -> Self {
        Self::from_flat(FlatUnits::ascii_with(length, |output| {
            let mut written = 0;
            for part in parts {
                output[written..written + part.len()].write_copy_of_slice(part);
                written += part.len();
            }
            assert_eq!(written, output.len(), "the parts fill the string's length");
        }))
    }

    /// Whether `length` units are copied whole into a joined string (`concat_all`).
    pub(in crate::vm) fn copies_whole(length: usize) -> bool {
        length <= ROPE_LEAF_MAX_UNITS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concatenation_plan_preserves_units_and_merges_adjacent_slices() {
        let source = JsString::from_units(&[0xD800, b'a' as u16, 0xDC00]);
        let mut plan = JsStringBuilder::default();
        plan.append_slice(&source, 0..1);
        plan.append_slice(&source, 1..3);
        assert_eq!(plan.parts.len(), 1);
        plan.append(&source);
        drop(source);
        assert_eq!(
            plan.finish().unwrap().units(),
            &[0xD800, b'a' as u16, 0xDC00, 0xD800, b'a' as u16, 0xDC00]
        );
    }
}
