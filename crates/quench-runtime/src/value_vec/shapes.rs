//! Shape lengths, slot owners, and the observed slot types that writes join while tracking.
use super::*;

impl ValueArena {
    pub(crate) fn register_shape(&mut self, shape: u32, length: usize) {
        self.shape_readers.retire(shape);
        assert!(shape < SHAPE_LIMIT, "object shape table exhausted");
        assert!(
            length <= u32::MAX as usize,
            "object has too many properties"
        );
        let shape = shape as usize;
        if self.shape_lengths.len() <= shape {
            self.shape_lengths.resize(shape + 1, 0);
        }
        self.shape_lengths[shape] = length as u32;
    }

    pub(crate) fn reset_shapes(&mut self) {
        self.widened_shapes.clear();
        self.shape_readers.reset();
        self.slot_readers.reset();
        self.tracking = false;
        self.shape_lengths.clear();
        self.shape_lengths.push(0);
        self.shape_chains.clear();
        self.chain_owners.clear();
        self.owner_types.clear();
        self.owner_epoch += 1;
    }

    /// Registers `shape`, of `length` slots, made from `parent` (of `parent_length`): one slot
    /// longer extends the parent's chain, owning the new slot, sharing the chain where the
    /// parent's is its end; as long shares it; anything else owns every slot itself.
    pub(crate) fn register_derived_shape(&mut self, shape: u32, length: usize, parent: Option<(u32, usize)>) {
        self.register_shape(shape, length);
        // Owners serve only kept representations; tracking's start registers every shape.
        if !self.tracking { return; }
        let index = shape as usize;
        if self.shape_chains.len() <= index {
            self.shape_chains.resize(index + 1, 0);
            self.owner_types.resize(index + 1, SlotType::None);
        }
        self.slot_readers.retire(shape);
        self.owner_types[index] = SlotType::None;
        self.owner_epoch += 1;
        // A parent's chain covers its length once registered; one that does not (a parent the
        // heap never registered at that length) lends no owners, and the child owns its slots.
        let covered = parent.filter(|(parent, parent_length)| self.shape_chains.get(*parent as usize)
            .is_some_and(|base| *base as usize + parent_length <= self.chain_owners.len()));
        let chain = match covered {
            Some((parent, parent_length)) if length == parent_length => self.shape_chains[parent as usize] as usize,
            Some((parent, parent_length)) if length == parent_length + 1 => {
                let base = self.shape_chains[parent as usize] as usize;
                if self.chain_owners.len() != base + parent_length {
                    let start = self.chain_owners.len();
                    self.chain_owners.extend_from_within(base..base + parent_length);
                    self.chain_owners.push(shape);
                    start
                } else {
                    self.chain_owners.push(shape);
                    base
                }
            }
            _ => {
                let start = self.chain_owners.len();
                self.chain_owners.extend(std::iter::repeat_n(shape, length));
                start
            }
        };
        self.shape_chains[index] = u32::try_from(chain).expect("slot owner chains fit u32");
    }

    /// Observes a write of `value` to `slot` of an object of `shape`, where the
    /// shape holds the slot (an ordinary shape's; a dictionary's holds none of its own).
    #[inline(always)] pub(crate) fn note_shape_value(&mut self, shape: u32, slot: usize, value: Value) {
        if self.tracking && slot < self.shape_length(shape) && !value.is_deleted() {
            self.note_slot_value(shape, slot, value);
        }
    }

    /// Observes an object's initial values, slot by slot.
    pub(crate) fn note_shape_values(&mut self, shape: u32, values: &[Value]) {
        if !self.tracking { return; }
        for (slot, value) in values.iter().copied().enumerate() {
            self.note_shape_value(shape, slot, value);
        }
    }

    /// Starts joining writes' classes from none: the caller joins what live objects hold.
    pub(crate) fn track(&mut self) {
        self.slot_readers.reset();
        self.tracking = true;
        self.owner_types.fill(SlotType::None);
        // Pending owners named the types just cleared: they widen nothing now.
        self.int_to_number.clear();
        self.owner_epoch += 1;
    }

    /// The epoch of the owners' types (`owner_epoch`): an `encodings_match` proof holds while it is current.
    #[inline(always)] pub(crate) fn owner_epoch(&self) -> u64 {
        self.owner_epoch
    }

    /// Whether the first `len` slots of an object of `source` shape and of `target` shape are both
    /// slots of their shapes, so a word of one shape's slot is the word of the other's: every slot
    /// holds the one `Value` word. The proof holds under the current `owner_epoch`.
    pub(crate) fn encodings_match(&self, source: u32, target: u32, len: usize) -> bool {
        self.shape_length(source) == len && self.shape_length(target) == len
    }

    /// Whether writes join their values' classes.
    #[inline] pub(crate) fn tracks(&self) -> bool {
        self.tracking
    }

    /// The slots `shape`'s objects store.
    #[inline]
    pub(crate) fn shape_length(&self, shape: u32) -> usize {
        self.shape_lengths.get(shape as usize).map_or(0, |length| *length as usize)
    }

    /// The shape that added `slot` of `shape`'s objects, whose representation it shares.
    #[inline(always)] pub(crate) fn slot_owner(&self, shape: u32, slot: usize) -> u32 {
        self.chain_owners[self.shape_chains[shape as usize] as usize + slot]
    }

    /// The type every value `slot` of `shape` holds is of, while tracking (`owner_types`).
    #[inline(always)] pub(crate) fn slot_observation(&self, shape: u32, slot: usize) -> SlotType {
        self.owner_types[self.slot_owner(shape, slot) as usize]
    }

    /// The word a store of `value` writes at `slot` of exact shape `shape` (decision 4.1, R4-M1): the
    /// raw double bits where the slot's owner holds the Number type (`raw_number_word`), `value` itself
    /// otherwise. Called after the store observed `value`, so a widening has already moved the owner.
    #[inline(always)] pub(crate) fn stored_word(&self, shape: u32, slot: usize, value: Value) -> Value {
        if self.tracking && self.slot_observation(shape, slot) == SlotType::Number { raw_number_word(value) } else { value }
    }

    /// `values` as the words of `shape`'s slots `0..values.len()` (`stored_word`), borrowed where no
    /// slot of them holds the Number type.
    pub(crate) fn stored_words<'a>(&self, shape: u32, values: &'a [Value]) -> std::borrow::Cow<'a, [Value]> {
        if !self.tracking { return std::borrow::Cow::Borrowed(values); }
        let number = |slot: usize| self.slot_observation(shape, slot) == SlotType::Number;
        if !(0..values.len()).any(|slot| number(slot)) { return std::borrow::Cow::Borrowed(values); }
        std::borrow::Cow::Owned(values.iter().enumerate().map(|(slot, &value)| if number(slot) { raw_number_word(value) } else { value }).collect())
    }

    /// Observe the next departure from this shape, not the contents of its slots.
    /// The caller must establish its target-shape assumption before subscribing.
    pub(crate) fn subscribe_shape(&mut self, shape: u32,
        code: &std::rc::Rc<dyn crate::code_dependencies::DependentCode>) -> bool {
        if shape as usize >= self.shape_lengths.len() { return false; }
        self.shape_readers.subscribe(shape, code)
    }

    #[inline(always)] pub(crate) fn note_shape_transition(&mut self, previous: u32, next: u32) {
        if previous != next {
            self.shape_readers.retire(previous);
        }
    }

    /// Retires the readers of `shape`, the class an object leaves when it departs its class cell
    /// (`Heap::depart_class_object`): `note_shape_transition` retires them on any other change.
    #[inline(always)] pub(crate) fn note_class_departure(&mut self, shape: u32) {
        self.shape_readers.retire(shape);
    }

    /// Joins `value`'s class into `slot` of `shape`'s representation; whether it widened. Only a
    /// widening is out of line (`widen_owner`): an untracked store is the test alone.
    #[inline(always)] pub(crate) fn note_slot_value(&mut self, shape: u32, slot: usize, value: Value) -> bool {
        self.tracking && { let owner = self.slot_owner(shape, slot); self.join_slot_owner(owner, value) }
    }

    #[inline(never)]
    pub(super) fn widen_owner(&mut self, owner: u32, class: SlotType) -> bool {
        self.owner_epoch += 1;
        self.slot_readers.retire(owner);
        // The words its slots hold are int-tagged: the heap rewrites them as raw doubles (decision 4.1).
        if self.owner_types[owner as usize] == SlotType::Int32 && class == SlotType::Number {
            self.int_to_number.push(owner);
        }
        self.owner_types[owner as usize] = class;
        true
    }

    /// Whether an Int32 owner widened to Number and its words are not rewritten yet.
    #[inline(always)] pub(crate) fn has_widened_numbers(&self) -> bool {
        !self.int_to_number.is_empty()
    }

    /// The owners that widened from Int32 to Number since the last call, sorted and unique.
    pub(crate) fn take_widened_numbers(&mut self) -> Vec<u32> {
        let mut owners = std::mem::take(&mut self.int_to_number);
        owners.sort_unstable();
        owners.dedup();
        owners
    }

    /// `len` values of the run at `start`, for storage that holds them so (record slots are tagged words).
    pub(super) fn decoded_run(&self, start: usize, len: usize) -> Vec<Value> {
        self.values[start..start + len].to_vec()
    }

    /// Admits the possible writes of a no-reentry raw slot capability before exposing it.
    pub(crate) fn note_slot_type(&mut self, shape: u32, slot: usize, ty: crate::typed_ir::Type) -> bool {
        if !self.tracking { return false; }
        let owner = self.slot_owner(shape, slot);
        self.join_owner_type(owner, || SlotType::from_type(ty))
    }

    /// Joins `incoming` into the slot type `owner` holds; whether it widened.
    #[inline(always)]
    pub(super) fn join_owner_type(&mut self, owner: u32, incoming: impl FnOnce() -> SlotType) -> bool {
        debug_assert!((owner as usize) < self.owner_types.len());
        // SAFETY: owners come from `chain_owners`, which only names shapes `owner_types` covers.
        let held = unsafe { *self.owner_types.get_unchecked(owner as usize) };
        if held == SlotType::Tagged { return false; }
        let class = held.join(incoming());
        class != held && self.widen_owner(owner, class)
    }
}
