//! Array builtins over dense backings: `concat` of arrays no `@@isConcatSpreadable` reaches,
//! and the present own elements a result is made of, read straight from the backing.
use super::*;

impl<H: Host> Vm<H> {
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

    /// `concat` when no step of it can be observed: ArraySpeciesCreate is this realm's
    /// ArrayCreate, and each item is a primitive, appended as one value, or an Array no
    /// `@@isConcatSpreadable` reaches whose elements are present own data elements, spread.
    /// The result is made at once from the values, as `slice`'s is; `None` leaves the
    /// algorithm to run step by step.
    pub(super) fn array_concat_dense(&mut self, source: Value, args: &[Value]) -> Option<Value> {
        if !self.array_species_is_array_create(source) {
            return None;
        }
        let symbol = self.well_known_symbols.get(WellKnownSymbol::IsConcatSpreadable)?;
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
        // An empty range (`slice(4, 3)`) appends nothing: every index in it is present.
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
            return !values.iter().any(|value| value.is_deleted());
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
    /// the prototype layout epoch does: any change to either prototype's shape or prototype
    /// advances it.
    pub(super) fn concat_spreadable_absent(&mut self, item: Value, symbol: Value) -> bool {
        let Some(array) = self.heap.array(item) else {
            return false;
        };
        if array.is_arguments_object() || array.has_side_descriptors() || array.proto() != self.array_proto {
            return false;
        }
        let (shape, epoch) = (array.shape(), crate::heap::prototype_layout_epoch());
        if self.concat_spreadable_proof == Some((shape, epoch)) {
            return true;
        }
        self.prove_concat_spreadable_absent(shape, symbol, epoch)
    }

    #[cold]
    #[inline(never)]
    fn prove_concat_spreadable_absent(&mut self, shape: u32, symbol: Value, epoch: u64) -> bool {
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
        self.heap.mark_prototype(array_proto);
        self.heap.mark_prototype(object_proto);
        // Marking an object that was no prototype yet leaves the epoch where it was.
        if crate::heap::prototype_layout_epoch() == epoch {
            self.concat_spreadable_proof = Some((shape, epoch));
        }
        true
    }

    /// Whether `value` is this realm's `Array`, the constructor ArrayCreate stands for.
    pub(super) fn realm_array_constructor(&self, value: Value) -> bool {
        matches!(
            self.heap.function(value),
            Some(FunctionRecord { kind: FunctionKind::Native(Native::Array), realm, .. }) if *realm == self.realm.globals
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
