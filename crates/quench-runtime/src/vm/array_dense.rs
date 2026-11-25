//! Array builtins over dense backings: `concat` of arrays no `@@isConcatSpreadable` reaches,
//! and the present own elements a result is made of, read straight from the backing.
use super::*;

impl Vm {
    /// IsConcatSpreadable of `item`, `symbol` being `@@isConcatSpreadable`.
    pub(super) fn concat_item_spreadable(&mut self, p: &ResidualProgram, item: Value, symbol: Value) -> Result<bool, JsError> {
        if self.concat_spreadable_absent(item, symbol) {
            // An Array no `@@isConcatSpreadable` reaches: the Get reads `undefined` and
            // IsArray holds.
            return Ok(true);
        }
        if !self.is_object_like(item) {
            return Ok(false);
        }
        let value = self.get_index(p, item, symbol)?;
        if value.is_undefined() { self.is_array(p, item) } else { Ok(self.truthy(value)) }
    }

    /// `Array.prototype.slice` in place: an array receiver whose species is this realm's
    /// ArrayCreate, bounds that are undefined or immediate numbers (their ToNumber runs no
    /// guest code), and a range of present own data elements. The result is one new array
    /// of those values; `None` leaves every other call to `array_slice_native`.
    pub fn plain_array_slice(&mut self, receiver: Value, args: &[Value]) -> Option<Value> {
        let length = self.own_array_length(receiver)?;
        let (start, end) = (slice_bound(args.first().copied(), 0, length)?, slice_bound(args.get(1).copied(), length, length)?);
        if !self.array_species_is_array_create(receiver) {
            return None;
        }
        self.array_of_dense_range(receiver, start..end.max(start))
    }

    /// A new array of the elements `range` of the dense array `source`, copied from its
    /// backing straight into the new array's: `None` where an index in `range` is not a present
    /// own data element.
    pub(super) fn array_of_dense_range(&mut self, source: Value, range: std::ops::Range<usize>) -> Option<Value> {
        if range.is_empty() {
            return Some(self.array_of(&[]));
        }
        let elements = self.array_search_elements(source).filter(|elements| range.end <= elements.len())?;
        if let Some(values) = elements.wide_values() {
            let values = &values[range];
            // A backing whose every value is an int32 holds no hole: the count proves it.
            if !elements.holds_only_int32() && values.iter().any(|value| value.is_deleted()) {
                return None;
            }
            let (base, len) = (values.as_ptr(), values.len());
            // SAFETY: `source`'s backing is its own allocation or a nursery run behind the
            // cursor; making the new array's run bumps the cursor past it and frees, moves and
            // writes nothing of it, and no collection runs before the copy ends.
            let part = unsafe { std::slice::from_raw_parts(base, len) };
            return Some(self.heap.alloc_literal_array_from_parts(self.array_proto, &[part]));
        }
        if let Some(words) = elements.narrow_words() {
            let words = &words[range];
            let (base, len) = (words.as_ptr(), words.len());
            // SAFETY: as for a wide backing.
            return Some(self.heap.alloc_literal_array(self.array_proto, len, |index| Value::from_narrow(unsafe { *base.add(index) })));
        }
        let mut values = smallvec::SmallVec::<[Value; 16]>::new();
        self.extend_present_dense(source, range, &mut values).then(|| self.array_of(&values))
    }

    /// `concat` when no step of it can be observed: ArraySpeciesCreate is this realm's
    /// ArrayCreate, and each item is a primitive, appended as one value, or an Array no
    /// `@@isConcatSpreadable` reaches whose elements are present own data elements, spread.
    /// The result is made at once from the values, as `slice`'s is; `None` leaves the
    /// algorithm to run step by step.
    pub fn array_concat_dense(&mut self, source: Value, args: &[Value]) -> Option<Value> {
        if !self.array_species_is_array_create(source) {
            return None;
        }
        let symbol = self.well_known_symbols.get(WellKnownSymbol::IsConcatSpreadable)?;
        // Each item as one segment: a primitive is itself, an array its wide or narrow
        // backing, read in place when the result is made (`ConcatSegment`).
        let mut segments = smallvec::SmallVec::<[ConcatSegment; 4]>::new();
        let mut total = 0usize;
        for &item in std::slice::from_ref(&source).iter().chain(args) {
            let segment = match self.concat_segment(item, symbol) {
                Some(segment) => segment,
                None if !self.is_object_like(item) => ConcatSegment::Value(item),
                None => return self.array_concat_decoded(source, args, symbol),
            };
            total += segment.len();
            segments.push(segment);
        }
        (total <= MAX_ARRAY_LENGTH).then(|| self.array_of_segments(&segments, total))
    }

    /// A new array of `segments`' `total` values, one allocation of the exact length: each value
    /// is read from its backing as the result's run is filled, with no intermediate buffer.
    fn array_of_segments(&mut self, segments: &[ConcatSegment], total: usize) -> Value {
        if segments.iter().all(|segment| segment.values().is_some()) {
            let mut parts = smallvec::SmallVec::<[&[Value]; CONCAT_INLINE_PARTS]>::new();
            parts.extend(segments.iter().filter_map(|segment| segment.values()));
            return self.heap.alloc_literal_array_from_parts(self.array_proto, &parts);
        }
        let mut reader = ConcatReader { segments, segment: 0, offset: 0 };
        self.heap.alloc_literal_array(self.array_proto, total, |_| reader.next_value())
    }

    /// `item`'s segment of a `concat` result in one look at its record: an ordinary Array of
    /// this realm with no extras (so neither sparse nor of another length), no indexed or side
    /// descriptors, whose shape is proved free of `@@isConcatSpreadable`
    /// (`concat_spreadable_absent`), and a wide backing without holes or a narrow one. `None`
    /// for anything else, which the caller sorts out.
    #[inline(always)]
    fn concat_segment(&mut self, item: Value, symbol: Value) -> Option<ConcatSegment> {
        let array = self.heap.array(item)?;
        if array.has_extras() || array.is_arguments_object() || array.has_indexed_descriptors() || array.has_side_descriptors() || array.proto() != self.array_proto {
            return None;
        }
        let shape = array.shape();
        let elements = array.elements();
        let segment = if let Some(values) = elements.wide_values() {
            if !elements.holds_only_int32() && values.iter().any(|value| value.is_deleted()) {
                return None;
            }
            ConcatSegment::Wide(values.as_ptr(), values.len())
        } else {
            let words = elements.narrow_words()?;
            ConcatSegment::Narrow(words.as_ptr(), words.len())
        };
        self.concat_spreadable_absent_shape(shape, symbol).then_some(segment)
    }

    /// `array_concat_dense` where an item's backing is neither wide nor narrow (doubles):
    /// every item decoded into one buffer, then copied into the result.
    #[cold]
    #[inline(never)]
    fn array_concat_decoded(&mut self, source: Value, args: &[Value], symbol: Value) -> Option<Value> {
        let mut values = smallvec::SmallVec::<[Value; 16]>::new();
        for item in std::iter::once(source).chain(args.iter().copied()) {
            if !self.is_object_like(item) {
                values.push(item);
                continue;
            }
            if !self.concat_spreadable_absent(item, symbol) {
                return None;
            }
            let length = self.own_array_length(item)?;
            if !self.extend_present_dense(item, 0..length, &mut values) {
                return None;
            }
        }
        (values.len() <= MAX_ARRAY_LENGTH).then(|| self.array_of(&values))
    }

    /// `present_dense_elements` of `range` appended to `out`, decoded from the backing in place
    /// rather than copied into a vector first; `false` (with `out` partly grown) where an index
    /// in `range` is not a present own data element.
    pub(super) fn extend_present_dense<const N: usize>(&self, this: Value, range: std::ops::Range<usize>, out: &mut smallvec::SmallVec<[Value; N]>) -> bool {
        if range.start >= range.end {
            return true;
        }
        let Some(elements) = self.array_search_elements(this).filter(|elements| range.end <= elements.len()) else {
            return false;
        };
        // A wide backing is copied and a narrow one decoded word by word, which holds no
        // hole; only another form reads value by value.
        if let Some(values) = elements.wide_values() {
            let values = &values[range];
            append_values(out, values.len(), |index| values[index]);
            return elements.holds_only_int32() || !values.iter().any(|value| value.is_deleted());
        }
        if let Some(words) = elements.narrow_words() {
            let words = &words[range];
            append_values(out, words.len(), |index| Value::from_narrow(words[index]));
            return true;
        }
        let start = out.len();
        out.extend(elements.range(range));
        !out[start..].iter().any(|value| value.is_deleted())
    }

    /// Whether `item` is an ordinary Array of this realm that no `@@isConcatSpreadable` reaches:
    /// neither its own shape nor `Array.prototype` nor `Object.prototype` (the whole chain) has
    /// the key. Then `Get(item, @@isConcatSpreadable)` reads `undefined` without running guest
    /// code and IsArray holds, so `concat` spreads it. A proof for the item's shape holds while
    /// the cell of the two prototypes does: a change to either one's layout retires it.
    pub(super) fn concat_spreadable_absent(&mut self, item: Value, symbol: Value) -> bool {
        let Some(array) = self.heap.array(item) else {
            return false;
        };
        if array.is_arguments_object() || array.has_side_descriptors() || array.proto() != self.array_proto {
            return false;
        }
        let shape = array.shape();
        self.concat_spreadable_absent_shape(shape, symbol)
    }

    /// `concat_spreadable_absent` for an ordinary Array of `shape`: the recorded proof for that
    /// shape (`concat_spreadable_proof`) answers while its cell holds, else the proof is made.
    #[inline(always)]
    fn concat_spreadable_absent_shape(&mut self, shape: u32, symbol: Value) -> bool {
        self.concat_spreadable_proof.is_some_and(|(proved, cell)| proved == shape && cell.holds())
            || self.prove_concat_spreadable_absent(shape, symbol)
    }

    #[cold]
    #[inline(never)]
    fn prove_concat_spreadable_absent(&mut self, shape: u32, symbol: Value) -> bool {
        let key = PropertyKey::symbol(symbol);
        if self.shape_is_dictionary(shape) || self.property_shape_slot(shape, key).is_some() {
            return false;
        }
        let (array_proto, object_proto) = (self.array_proto, self.object_proto);
        let lacks = |vm: &Self, holder: Value, parent: Value| {
            vm.object_data(holder).is_some_and(|data| {
                data.proto() == parent && !vm.shape_is_dictionary(data.shape()) && vm.property_shape_slot(data.shape(), key).is_none()
            })
        };
        if !lacks(self, array_proto, object_proto) || !lacks(self, object_proto, Value::NULL) {
            return false;
        }
        // Marking an object that was no prototype yet changes no layout, so the finding holds
        // under the cell of the two made after it: `Array.prototype`, then `Object.prototype`,
        // its prototype.
        if let Some(cell) = self.chain_cell(array_proto, 2) {
            self.concat_spreadable_proof = Some((shape, cell));
        }
        true
    }

    /// Whether `value` is this realm's `Array`, the constructor ArrayCreate stands for.
    pub fn realm_array_constructor(&self, value: Value) -> bool {
        matches!(
            self.heap.function(value),
            Some(record @ FunctionRecord { kind: FunctionKind::Native(Native::Array), .. }) if self.record_realm(record) == self.realm.globals
        )
    }
}

/// Appends the `len` values `value` gives for each index to `out`, after one reservation:
/// SmallVec's `extend` and `extend_from_slice` check capacity or shift per call.
#[inline(always)]
fn append_values<const N: usize>(out: &mut smallvec::SmallVec<[Value; N]>, len: usize, value: impl Fn(usize) -> Value) {
    out.reserve(len);
    let start = out.len();
    // SAFETY: the reservation holds `len` values past `start`, each written before `set_len`
    // admits it; a `Value` is `Copy`, so nothing is dropped.
    unsafe {
        let base = out.as_mut_ptr().add(start);
        for index in 0..len {
            base.add(index).write(value(index));
        }
        out.set_len(start + len);
    }
}

/// A `slice` bound in place: `default` for an absent or undefined argument, an int32 clamped
/// as `relative_index` clamps it, any other immediate number through `relative_index`; `None`
/// for a value whose ToNumber could run guest code.
#[inline(always)]
fn slice_bound(value: Option<Value>, default: usize, length: usize) -> Option<usize> {
    let Some(value) = value.filter(|value| !value.is_undefined()) else {
        return Some(default);
    };
    match value.as_int() {
        Some(index) => Some(crate::staged::string_search::relative_bound(i64::from(index), length)),
        None => Some(crate::units::relative_index(value.immediate_number()?, length)),
    }
}

/// One item of `array_concat_dense`'s result: a primitive, or a dense array's backing, wide
/// or narrow, with its length.
#[derive(Clone, Copy)]
enum ConcatSegment {
    Value(Value),
    Wide(*const Value, usize),
    Narrow(*const u32, usize),
}

/// The segments `array_of_segments` holds inline before a `concat` result's parts spill to the heap.
const CONCAT_INLINE_PARTS: usize = 4;

/// The values of a `concat` result in order, read from its segments: each call gives the next.
struct ConcatReader<'a> {
    segments: &'a [ConcatSegment],
    segment: usize,
    offset: usize,
}

impl ConcatReader<'_> {
    /// The next value of the result, moving past the segments already read through.
    #[inline(always)]
    fn next_value(&mut self) -> Value {
        while self.offset >= self.segments[self.segment].len() {
            self.segment += 1;
            self.offset = 0;
        }
        let value = self.segments[self.segment].get(self.offset);
        self.offset += 1;
        value
    }
}

impl ConcatSegment {
    /// The segment as a slice of plain values: a primitive's one value, or a wide backing's
    /// values. `None` for a narrow backing, whose words decode one by one.
    #[inline(always)]
    fn values(&self) -> Option<&[Value]> {
        match self {
            Self::Value(value) => Some(std::slice::from_ref(value)),
            // SAFETY: as for `get`: the backing is still the array's, read before any write or
            // collection, and the result's allocation is written with no collection.
            Self::Wide(base, len) => Some(unsafe { std::slice::from_raw_parts(*base, *len) }),
            Self::Narrow(..) => None,
        }
    }

    /// The value at `offset`, below `len`, of this segment.
    #[inline(always)]
    fn get(self, offset: usize) -> Value {
        // SAFETY: `offset` is below the backing's length, and each backing is still the array's:
        // nothing ran since it was read, and the result's run is written with no collection.
        match self {
            Self::Value(value) => value,
            Self::Wide(base, _) => unsafe { *base.add(offset) },
            Self::Narrow(base, _) => Value::from_narrow(unsafe { *base.add(offset) }),
        }
    }

    /// The values this segment adds to the result.
    #[inline(always)]
    fn len(self) -> usize {
        match self {
            Self::Value(_) => 1,
            Self::Wide(_, len) | Self::Narrow(_, len) => len,
        }
    }
}
