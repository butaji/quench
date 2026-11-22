//! The lane's for-of and destructuring steps of built-in iterators, made in place: one read
//! of the iterator's record decides what the step is and, for an array values iterator,
//! gives the position the step reads, with no result object and no second read of the
//! record.
use super::*;

/// What an iteration step calling `method` of an iterator is (`Vm::step_in_place`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InPlaceStep {
    /// A values step over a plain array, made: the element, or `None` once the iterator is
    /// done. It allocated nothing.
    Element(Option<Value>),
    /// A String, Map or Set step, or an array keys or entries step, made: the value, or
    /// `None` once the iterator is done. It may have allocated the value.
    Item(Option<Value>),
    /// This realm's `Generator.prototype.next` of a generator that is not running
    /// (`IterationStep::Generator`), not yet resumed.
    Generator,
    /// Any other step, and a built-in one whose step runs as the native's (a hole, an exotic
    /// or sparse array, a done iterator): nothing is made.
    Call,
}

impl Vm {
    /// The step calling `method` of `iterator`, made where it runs no guest code
    /// (`InPlaceStep`). An array values step is decided and made from one read of the
    /// record; the other kinds are decided as `iteration_step` decides them.
    #[inline(always)]
    pub fn step_in_place(&mut self, iterator: Value, method: Value) -> InPlaceStep {
        let Some(record) = self.heap.iterator(iterator) else {
            return InPlaceStep::Call;
        };
        if matches!(record.kind, IteratorKind::Array | IteratorKind::ArrayValues) {
            let (done, source, position) = (record.done, record.source, record.ext.position(record.index));
            if done || !self.is_current_realm_native(method, Native::ArrayIteratorNext) {
                return InPlaceStep::Call;
            }
            return self.step_present_element(iterator, source, position).map_or(InPlaceStep::Call, InPlaceStep::Element);
        }
        match self.iteration_step(iterator, method) {
            IterationStep::Collection => self.step_collection(iterator).map_or(InPlaceStep::Call, InPlaceStep::Item),
            IterationStep::Generator => InPlaceStep::Generator,
            IterationStep::Array | IterationStep::Call => InPlaceStep::Call,
        }
    }

    /// One %ArrayIteratorPrototype%.next step of a values iterator not yet done over
    /// `source` at `position`, where it runs no guest code (`step_present_array_value`): the
    /// present data element at the position, the iterator left past it (`Some(Some(_))`),
    /// or the iterator done past the end (`Some(None)`). `None` for a hole some prototype
    /// may fill, or a source that is no plain array, or one with indexed descriptors, an
    /// arguments mapping or sparse elements.
    #[inline(always)]
    pub(super) fn step_present_element(&mut self, iterator: Value, source: Value, position: usize) -> Option<Option<Value>> {
        let array = self.heap.array(source)?;
        if array.has_indexed_descriptors() || array.is_arguments_object() || self.heap.sparse_length(source).is_some() {
            return None;
        }
        let Some(value) = array.elements().get(position) else {
            if let Some(IteratorRecord { done, source, .. }) = self.heap.iterator_mut(iterator) {
                *done = true;
                *source = Value::UNDEFINED;
            }
            return Some(None);
        };
        // A hole's Get reaches the prototype chain: `undefined` where neither the array nor
        // any prototype holds the index (`absent_array_element`, the chain's proof past the
        // array), else the native's step.
        let value = if value.is_deleted() { self.absent_array_element(source, Value::number(position as f64))? } else { value };
        self.heap.store_iterator_position(iterator, position + 1);
        Some(Some(value))
    }

    /// Whether a Get of any hole of plain array `array` reads `undefined`: neither its own
    /// shape nor any prototype holds an index (`prototypes_hold_no_index`, the chain's proof).
    pub(super) fn holes_read_undefined(&self, array: Value) -> bool {
        self.heap.array(array).is_some_and(|data| !self.shape_has_index_key(data.shape())) && self.prototypes_hold_no_index(array)
    }
}
