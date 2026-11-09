//! for-in's key walk: the key snapshot a loop takes (`Vm::for_in_start`), the state
//! `Op::ForInNext` steps through it (`Vm::for_in_next`), and the shape memo that lets a step
//! skip its key's lookup (`ForInOwnKeys`).

use super::*;
use crate::heap::ArrayElements;
use crate::vm::enumerable_key_memo::EnumerableSlots;

/// What for-in keeps between loops: the last key snapshot (`ForInOwnKeys`), and the state of
/// the last loop that ran out of keys with the collection epoch then (`Vm::for_in_state`).
#[derive(Default)]
pub(in crate::vm) struct ForInMemo {
    own_keys: Option<ForInOwnKeys>,
    spare_state: Option<(Value, u64)>,
}

impl ForInMemo {
    /// Drops the snapshot, whose shape a compaction renumbers.
    pub(in crate::vm) fn forget_snapshot(&mut self) {
        self.own_keys = None;
    }
}

/// The key snapshot a for-in last took from a shape (`Vm::list_own_for_in_keys`): the internal
/// array of that shape's own enumerable keys, with the shape and the collection epoch. In
/// the same epoch (which no reused handle or compacted shape shares), any receiver with that
/// shape and no side descriptors has exactly these keys, each an own enumerable property, so
/// a later loop over such a receiver whose prototypes are still the ones listed, each of its
/// listed shape, reuses the array (`Vm::listed_for_in_keys`), and a step of a loop walking it
/// over such a receiver needs no lookup (`Vm::for_in_snapshot_current`). No script reaches
/// the array, so loops share it.
struct ForInOwnKeys {
    shape: u32,
    epoch: u64,
    keys: Value,
    /// Each key's atom and slot in `shape`, in snapshot order.
    slots: EnumerableSlots,
    /// The receiver's prototypes when listed, each with its shape: none had an enumerable key.
    chain: Box<[(Value, u32)]>,
    /// The key the last step over a receiver of `shape` yielded, with its snapshot index: a
    /// read of that key from such a receiver is a slot read (`Vm::enumerated_key_get`), as
    /// V8 loads a property with an enumerated key.
    stepped: Option<(Value, usize)>,
}

impl ForInOwnKeys {
    /// The snapshot array, while it holds the keys of `shape` in `epoch`.
    fn keys(&self, shape: u32, epoch: u64) -> Option<Value> {
        (self.shape == shape && self.epoch == epoch).then_some(self.keys)
    }
}

/// A for-in state's elements: the (boxed) object, its key snapshot and the cursor, the
/// index of the snapshot key the next step reads.
const OBJECT: usize = 0;
const KEYS: usize = 1;
const CURSOR: usize = 2;

impl<H: Host> Vm<H> {
    /// The state a for-in over `source` steps (`Op::ForInNext`): its object and key snapshot,
    /// at the first key. `null` and `undefined` list no key.
    pub(in crate::vm) fn for_in_start(&mut self, p: &ResidualProgram, source: Value) -> Result<Value, JsError> {
        if source.is_null() || source.is_undefined() {
            let keys = self.array_of(&[]);
            return Ok(self.for_in_state(Value::UNDEFINED, keys));
        }
        let object = self.box_object(source)?;
        if let Some(keys) = self.listed_for_in_keys(object) {
            return Ok(self.for_in_state(object, keys));
        }
        let root = self.heap.root(object);
        let keys = self.object_for_in_keys(p, object);
        let object = self.heap.root_value(root).expect("rooted for-in object");
        self.heap.release_root(root);
        Ok(self.for_in_state(object, keys?))
    }

    /// The for-in state over `object` and its key snapshot `keys`, at the first key. Only its
    /// loop's steps read a state, and none after it runs out of keys (`Vm::for_in_next`), so a
    /// state spared then whose record no collection has passed since is taken over instead
    /// of allocating one.
    fn for_in_state(&mut self, object: Value, keys: Value) -> Value {
        let elements = [object, keys, Value::integer(0)];
        let epoch = self.heap.collection_epoch();
        if let Some((state, _)) = self.for_in.spare_state.take().filter(|&(_, spared)| spared == epoch) {
            let values = self.heap.array_mut(state).expect("spared for-in state").elements_mut().make_mut();
            elements.into_iter().enumerate().for_each(|(index, value)| values.set(index, value));
            return state;
        }
        // Its own backing, not a young run, which the cursor's first write would copy.
        let object = crate::heap::ArrayObject::fresh(self.array_proto, ArrayElements::from_slice(&elements));
        self.heap.alloc(Cell::Array { object })
    }

    /// The next key of the for-in `state` that is still an enumerable property of its
    /// object, past any deleted or made non-enumerable since the snapshot
    /// (EnumerateObjectProperties); `None` once the snapshot is exhausted.
    pub(in crate::vm) fn for_in_next(&mut self, p: &ResidualProgram, state: Value) -> Result<Option<Value>, JsError> {
        while let Some((object, keys, key, cursor)) = self.for_in_peek(state) {
            self.for_in_advance(state, cursor);
            if self.for_in_snapshot_current(object, keys) {
                if let Some(listed) = self.for_in.own_keys.as_mut() {
                    listed.stepped = Some((key, cursor));
                }
                return Ok(Some(key));
            }
            let root = self.heap.root(key);
            let enumerable = self.object_for_in_key_is_enumerable(p, object, key);
            let key = self.heap.root_value(root).expect("rooted for-in key");
            self.heap.release_root(root);
            if enumerable? {
                return Ok(Some(key));
            }
        }
        self.for_in.spare_state = Some((state, self.heap.collection_epoch()));
        Ok(None)
    }

    /// The state's object, snapshot, key at the cursor and cursor; `None` past the last key.
    #[inline(always)]
    fn for_in_peek(&self, state: Value) -> Option<(Value, Value, Value, usize)> {
        let elements = self.heap.array(state)?.elements();
        let cursor = usize::try_from(elements.get(CURSOR)?.as_int()?).ok()?;
        let (object, keys) = (elements[OBJECT], elements[KEYS]);
        let key = *self.heap.array(keys)?.elements().get(cursor)?;
        Some((object, keys, key, cursor))
    }

    /// Moves the state's cursor past `cursor`.
    #[inline(always)]
    fn for_in_advance(&mut self, state: Value, cursor: usize) {
        let next = Value::integer(i32::try_from(cursor + 1).expect("for-in snapshot fits an index"));
        let array = self.heap.array_mut(state).expect("for-in state");
        if !array.replace_in_place(CURSOR, next) {
            array.elements_mut().make_mut().set(CURSOR, next);
        }
    }

    /// Whether `keys` is the snapshot of `object`'s current shape (`ForInOwnKeys`) while it
    /// stores no attributes beside it: then each of its keys is still an own enumerable
    /// property of `object`, as a step asks, without a lookup.
    #[inline(always)]
    fn for_in_snapshot_current(&self, object: Value, keys: Value) -> bool {
        let Some(listed) = self.for_in.own_keys.as_ref() else { return false };
        self.object_data(object)
            .filter(|data| !data.has_side_descriptors())
            .and_then(|data| listed.keys(data.shape(), self.heap.collection_epoch()))
            == Some(keys)
    }

    /// `object` as an ordinary object whose own keys and attributes its shape alone decides:
    /// no side descriptors, module namespace or boxed primitive.
    #[inline(always)]
    fn shape_keyed_object(&self, object: Value) -> Option<&Object> {
        self.heap.ordinary_object(object).filter(|data| {
            !data.has_side_descriptors()
                && !data.is_module_namespace()
                && data.boxed_primitive().is_none()
                && data.deferred_module().is_none()
        })
    }

    /// The memo's snapshot (`ForInOwnKeys`) for `receiver`: a shape-keyed object of its shape
    /// whose prototypes are the ones listed, each still of its listed shape, in the same
    /// epoch. None of them has an enumerable key then, as when listed.
    pub(super) fn listed_for_in_keys(&self, receiver: Value) -> Option<Value> {
        let listed = self.for_in.own_keys.as_ref()?;
        let data = self.shape_keyed_object(receiver)?;
        let keys = listed.keys(data.shape(), self.heap.collection_epoch())?;
        let mut prototype = data.proto();
        for &(walked, shape) in &listed.chain {
            let data = self.object_data(prototype).filter(|_| prototype == walked)?;
            if data.shape() != shape || data.has_side_descriptors() {
                return None;
            }
            prototype = data.proto();
        }
        prototype.is_null().then_some(keys)
    }

    /// `shape_for_in_keys` of a receiver none of whose prototypes has an enumerable string
    /// key, as most have not (class methods are not enumerable): a prototype then neither
    /// adds a key nor shadows one the receiver lists first, so the keys are the receiver's
    /// own enumerable ones, read from the shape memo (`shape_enumerable_slots`) and
    /// snapshotted for the next loop (`ForInOwnKeys`). `None` leaves any other chain to the
    /// walk.
    pub(super) fn list_own_for_in_keys(&mut self, receiver: Value) -> Option<Value> {
        let slots = self.shape_enumerable_slots(receiver)?;
        let mut chain = Vec::new();
        let mut prototype = self.object_data(receiver)?.proto();
        while !prototype.is_null() {
            if !self.shape_enumerable_slots(prototype)?.is_empty() {
                return None;
            }
            let data = self.object_data(prototype)?;
            chain.push((prototype, data.shape()));
            prototype = data.proto();
        }
        let (shape, epoch) = (self.object_data(receiver)?.shape(), self.heap.collection_epoch());
        let keys: Vec<Value> = slots.iter().map(|&(atom, _)| self.atom_string(atom)).collect();
        let keys = self.array_of(&keys);
        self.for_in.own_keys = Some(ForInOwnKeys { shape, epoch, keys, slots, chain: chain.into(), stepped: None });
        Some(keys)
    }

    /// `base[key]` where `key` is the one the last for-in step over a receiver of `base`'s
    /// shape yielded (`ForInOwnKeys::stepped`): that key's slot in the shape, a data property
    /// as the shape has no accessor, holds the value. `None` for any other read.
    #[inline(always)]
    pub(in crate::vm) fn enumerated_key_get(&self, base: Value, key: Value) -> Option<Value> {
        let listed = self.for_in.own_keys.as_ref()?;
        let (_, index) = listed.stepped.filter(|&(stepped, _)| stepped == key)?;
        let data = self.shape_keyed_object(base)?;
        let shape = data.shape();
        listed.keys(shape, self.heap.collection_epoch())?;
        let (atom, slot) = *listed.slots.get(index)?;
        if self.shape_may_have_accessors(shape) || !self.plain_named_key(atom) {
            return None;
        }
        self.heap.property_get(data, slot as usize)
    }

    /// `base[key]` of a string `key` that names no array index, where the shapes alone answer:
    /// the slot of the key a for-in just stepped to (`enumerated_key_get`), or a read by its
    /// atom as a named read is (`plain_named_get`, which refuses index atoms). `None` for any
    /// other key, or a read those decline.
    #[inline(always)]
    pub(in crate::vm) fn plain_string_keyed_get(&self, base: Value, key: Value) -> Option<Value> {
        if let Some(value) = self.enumerated_key_get(base, key) {
            return Some(value);
        }
        self.plain_named_get(base, self.lookup_string_atom(key)?)
    }
}

impl<H: Host> Vm<H> {
    /// The array of for-in `keys` with `prototype`.
    pub(super) fn alloc_for_in_key_array(&mut self, keys: crate::heap::ArrayElements, prototype: Value) -> Value {
        self.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(Self::empty_object(prototype), keys),
        })
    }
}
