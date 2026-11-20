//! for-in's key walk: the key snapshot a loop takes (`Vm::for_in_start`), the state
//! `ForInNext` steps through it (`Vm::for_in_next`), and the shape memo that lets a step
//! skip its key's lookup (`ForInOwnKeys`).

use super::*;
use crate::heap::ArrayElements;
use crate::staged::iteration::{FOR_IN_CURSOR, FOR_IN_EPOCH, FOR_IN_KEYS, FOR_IN_NO_MEMO, FOR_IN_OBJECT, FOR_IN_SHAPE, FOR_IN_SLOTS, FOR_IN_STATE_LEN, concrete_for_in_next};
use crate::vm::enumerable_key_memo::EnumerableSlots;

/// What for-in keeps between loops: the last key snapshot (`ForInOwnKeys`), and the state of
/// the last loop that ran out of keys with the collection epoch then (`Vm::for_in_state`).
#[derive(Default)]
pub(in crate::vm) struct ForInMemo {
    own_keys: Option<ForInOwnKeys>,
    spare_state: Option<(Value, u64)>,
    /// The loop that stepped last: its state with its object, snapshot and snapshot length, so a
    /// step reads them without decoding the state. The cursor stays the state's own
    /// `FOR_IN_CURSOR` element, which compiled code steps too (`staged::iteration::for_in_next`).
    /// Every collection forgets it (`Vm::for_in_forget_active`).
    active: Option<ActiveForIn>,
    /// The shapes the last key walk of an ordinary chain saw (`Vm::shape_for_in_keys`).
    walked: ForInWalk,
}

#[derive(Clone, Copy)]
struct ActiveForIn {
    state: Value,
    object: Value,
    keys: Value,
    len: usize,
}

/// The chain a key snapshot was walked over, up to the object that owns its last enumerable key,
/// each object with its shape (`Vm::shape_for_in_keys`). While each still has its shape, its
/// prototype link and no side descriptors, and the collection epoch is the one recorded, every
/// key of `keys` is still an own enumerable property of the object the walk found it on, so a
/// step over it needs no lookup (`Vm::for_in_key_is_enumerable`). One buffer serves each walk.
#[derive(Default)]
struct ForInWalk {
    keys: Option<Value>,
    epoch: u64,
    chain: Vec<(Value, u32)>,
}

impl ForInWalk {
    fn record(&mut self, keys: Value, epoch: u64, chain: &[(Value, u32)]) {
        self.keys = Some(keys);
        self.epoch = epoch;
        self.chain.clear();
        self.chain.extend_from_slice(chain);
    }

    fn forget(&mut self) {
        self.keys = None;
    }
}

impl ForInMemo {
    /// Drops the snapshot, whose shape a compaction renumbers, and the walk's shapes with it.
    pub(in crate::vm) fn forget_snapshot(&mut self) {
        self.own_keys = None;
        self.walked.forget();
    }
}

/// The key snapshot a for-in last took from a shape (`Vm::record_receiver_for_in_keys`): the internal
/// array of that shape's own enumerable keys, with the shape and the collection epoch. In
/// the same epoch (which no reused handle or compacted shape shares), any receiver with that
/// shape and no side descriptors has exactly these keys, each an own enumerable property, so
/// a later loop over such a receiver whose prototype is still the one listed, with the chain
/// from it still holding its cell, reuses the array (`Vm::listed_for_in_keys`), and a step of a loop walking it
/// over such a receiver needs no lookup (`Vm::for_in_snapshot_current`). No script reaches
/// the array, so loops share it.
struct ForInOwnKeys {
    shape: u32,
    epoch: u64,
    keys: Value,
    /// The array of each snapshot key's slot in `shape` (`staged::iteration::FOR_IN_SLOTS`): an int,
    /// or `undefined` where no slot read answers the key (an index key, a shape with accessors).
    slot_words: Value,
    /// Each key's atom and slot in `shape`, in snapshot order.
    slots: EnumerableSlots,
    /// The receiver's prototype when listed.
    prototype: Value,
    /// The cell of the chain from `prototype` to its end (`Vm::chain_cell_to_end`), made when
    /// listed: it holds while no link changes its layout or dies, so each of them still has
    /// the shape it had then, with no side descriptors and no enumerable key.
    chain: crate::heap::ChainCell,
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

impl<H: Host> Vm<H> {
    /// The state a for-in over `source` steps (`ForInNext`): its object and key snapshot,
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

    /// `for_in_state` for the lane's `ForInStart` over an object the memo lists.
    pub(in crate::vm) fn for_in_state_in_lane(&mut self, object: Value, keys: Value) -> Value {
        self.for_in_state(object, keys)
    }

    /// The for-in state over `object` and its key snapshot `keys`, at the first key, with the
    /// memo of the snapshot's validity (`for_in_state_record`). Only its loop's steps read a
    /// state, and none after it runs out of keys (`Vm::for_in_next`), so a state spared then
    /// whose record no collection has passed since is taken over instead of allocating one.
    fn for_in_state(&mut self, object: Value, keys: Value) -> Value {
        let state = self.for_in_state_record(object, keys);
        // The new loop steps next: it becomes the active one.
        if let Some(len) = self.heap.array(keys).map(|keys| keys.elements().len()) {
            self.for_in.active = Some(ActiveForIn { state, object, keys, len });
        }
        state
    }

    /// The state record of a new loop: its object, snapshot and first cursor, with the memo of
    /// the snapshot's validity where `keys` is the one last listed (`staged::iteration`: its
    /// shape and epoch as raw words, else `FOR_IN_NO_MEMO`).
    fn for_in_state_record(&mut self, object: Value, keys: Value) -> Value {
        let (shape, listed_epoch, slot_words) = self.for_in.own_keys.as_ref().filter(|listed| listed.keys == keys)
            .map_or((FOR_IN_NO_MEMO, FOR_IN_NO_MEMO, Value::UNDEFINED),
                |listed| (u64::from(listed.shape), listed.epoch, listed.slot_words));
        let mut elements = [Value::integer(0); FOR_IN_STATE_LEN];
        elements[FOR_IN_OBJECT] = object;
        elements[FOR_IN_KEYS] = keys;
        elements[FOR_IN_SHAPE] = Value::from_bits(shape);
        elements[FOR_IN_EPOCH] = Value::from_bits(listed_epoch);
        elements[FOR_IN_SLOTS] = slot_words;
        let epoch = self.heap.collection_epoch();
        if let Some((state, _)) = self.for_in.spare_state.take().filter(|&(_, spared)| spared == epoch) {
            if self.for_in.active.is_some_and(|active| active.state == state) {
                self.for_in.active = None;
            }
            let values = self.heap.array_mut(state).expect("spared for-in state").elements_mut().make_mut();
            elements.into_iter().enumerate().for_each(|(index, value)| values.set(index, value));
            return state;
        }
        // Its own backing, not a young run, which the cursor's first write would copy.
        let object = crate::heap::ArrayObject::fresh(self.array_proto, ArrayElements::from_slice(&elements));
        self.heap.alloc_array(object)
    }

    /// The next key of the for-in `state` that is still an enumerable property of its
    /// object, past any deleted or made non-enumerable since the snapshot
    /// (EnumerateObjectProperties); `None` once the snapshot is exhausted.
    pub(in crate::vm) fn for_in_next(&mut self, p: &ResidualProgram, state: Value) -> Result<Option<Value>, JsError> {
        if let Some(step) = self.for_in_step_listed(state) {
            return Ok(step);
        }
        while let Some((object, keys, key, cursor)) = self.for_in_peek(state) {
            self.for_in_advance(state, cursor);
            let root = self.heap.root(key);
            let enumerable = self.for_in_key_is_enumerable(p, object, keys, key);
            let key = self.heap.root_value(root).expect("rooted for-in key");
            self.heap.release_root(root);
            if enumerable? {
                return Ok(Some(key));
            }
        }
        self.for_in.spare_state = Some((state, self.heap.collection_epoch()));
        Ok(None)
    }

    /// The step of the loop over `state` that needs no lookup: `Some(None)` once its snapshot
    /// is exhausted, `Some(Some(key))` for the next key of a snapshot that is current
    /// (`for_in_snapshot_current`), whose keys are all still own enumerable properties; `None`
    /// where each key needs its enumerability checked (`for_in_next`), which may reach a
    /// proxy's traps.
    #[inline(always)]
    pub(in crate::vm) fn for_in_step_listed(&mut self, state: Value) -> Option<Option<Value>> {
        if let Some(key) = self.for_in_staged_step(state) {
            return Some(Some(key));
        }
        let Some((object, keys, key, cursor)) = self.for_in_peek(state) else {
            self.for_in.spare_state = Some((state, self.heap.collection_epoch()));
            return Some(None);
        };
        if !self.for_in_snapshot_current(object, keys) {
            return None;
        }
        self.for_in_advance(state, cursor);
        if let Some(listed) = self.for_in.own_keys.as_mut() {
            listed.stepped = Some((key, cursor));
        }
        Some(Some(key))
    }

    /// The step the staged row makes (`staged::iteration::for_in_next`) where the state's memo proves
    /// the object still has the snapshot's keys: its key, the cursor moved past it, with no lookup.
    /// `None` leaves the step to the walk, which a deleted or non-enumerable key, another kind of
    /// object and the last key's exhaustion take.
    #[inline(always)]
    fn for_in_staged_step(&self, state: Value) -> Option<Value> {
        let (arrays, objects) = (self.heap.array_record_base()?, self.heap.object_record_base()?);
        concrete_for_in_next(arrays, objects, state, self.heap.collection_epoch())
    }

    /// The state's object, snapshot, key at the cursor and cursor; `None` past the last key.
    /// The loop becomes the active one (`ForInMemo::active`).
    #[inline(always)]
    fn for_in_peek(&mut self, state: Value) -> Option<(Value, Value, Value, usize)> {
        let cursor = self.heap.array(state)?.elements().get(FOR_IN_CURSOR)?.as_int()?;
        let cursor = usize::try_from(cursor).ok()?;
        if let Some(active) = self.for_in.active.filter(|active| active.state == state) {
            if cursor >= active.len {
                return None;
            }
            let key = self.heap.array(active.keys)?.elements().get(cursor)?;
            return Some((active.object, active.keys, key, cursor));
        }
        self.for_in_activate(state, cursor)
    }

    /// `for_in_peek` of a loop that is not the active one, at `cursor`: its object and snapshot
    /// read from its state.
    #[cold]
    #[inline(never)]
    fn for_in_activate(&mut self, state: Value, cursor: usize) -> Option<(Value, Value, Value, usize)> {
        let elements = self.heap.array(state)?.elements();
        let (object, keys) = (elements.get(FOR_IN_OBJECT)?, elements.get(FOR_IN_KEYS)?);
        let listed = self.heap.array(keys)?.elements();
        let len = listed.len();
        let key = listed.get(cursor)?;
        self.for_in.active = Some(ActiveForIn { state, object, keys, len });
        Some((object, keys, key, cursor))
    }

    /// Forgets the active loop, whose words a collection may move or free.
    pub(in crate::vm) fn for_in_forget_active(&mut self) {
        self.for_in.active = None;
    }

    fn for_in_store_cursor(&mut self, state: Value, cursor: usize) {
        let next = Value::integer(i32::try_from(cursor).expect("for-in snapshot fits an index"));
        let Some(array) = self.heap.array_mut(state) else { return };
        if !array.replace_in_place(FOR_IN_CURSOR, next) {
            array.elements_mut().make_mut().set(FOR_IN_CURSOR, next);
        }
    }

    /// Moves the state's cursor past `cursor`.
    #[inline(always)]
    fn for_in_advance(&mut self, state: Value, cursor: usize) {
        self.for_in_store_cursor(state, cursor + 1);
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
    /// whose prototype is the one listed, with the chain from it still holding its cell, in the
    /// same epoch. None of the prototypes has an enumerable key then, as when listed.
    pub(in crate::vm) fn listed_for_in_keys(&self, receiver: Value) -> Option<Value> {
        let listed = self.for_in.own_keys.as_ref()?;
        let data = self.shape_keyed_object(receiver)?;
        let keys = listed.keys(data.shape(), self.heap.collection_epoch())?;
        (data.proto() == listed.prototype && listed.chain.holds()).then_some(keys)
    }

    /// The for-in key snapshot of an ordinary `receiver` (`Vm::object_for_in_keys`), from one walk
    /// of its chain: every enumerable string key along the chain, each once, in
    /// EnumerateObjectProperties order. The walk stops at the last object with an enumerable key,
    /// since later objects can neither add a key nor shadow one; a name an earlier object owns
    /// shadows the same name further on, enumerable or not. The last object's keys come from its
    /// memoized slots (`shape_enumerable_slots`), the others' from their own slots
    /// (`for_in_ordinary_names`). When every object of the chain has that memo and the last object
    /// is the receiver or none is enumerable, the keys are the receiver's own, and the walk records
    /// them as the memo a later loop reuses (`ForInOwnKeys`). The shapes the walk saw are recorded
    /// for the snapshot (`ForInWalk`). `None` sends a chain with an object whose keys do not live
    /// in its shape to the generic walk.
    pub(super) fn shape_for_in_keys(&mut self, receiver: Value) -> Option<Value> {
        let mut chain = smallvec::SmallVec::<[(Value, u32); 4]>::new();
        let mut last = None;
        let mut receiver_slots = None;
        let mut memoized = true;
        let mut current = receiver;
        while !current.is_null() {
            if !self.shape_lists_enumerable_keys(current) {
                return None;
            }
            let data = self.object_data(current)?;
            let (shape, prototype) = (data.shape(), data.proto());
            let slots = self.shape_enumerable_slots(current);
            let enumerable = match &slots {
                Some(slots) => !slots.is_empty(),
                None => self.for_in_ordinary_names(current).iter().any(|&(_, enumerable)| enumerable),
            };
            if chain.is_empty() {
                receiver_slots = slots.clone();
            }
            memoized &= slots.is_some();
            if enumerable {
                last = Some(chain.len());
            }
            chain.push((current, shape));
            current = prototype;
        }
        // The walk's objects up to the last enumerable key decide each key's enumerability.
        let walked = last.map_or(0, |last| last + 1);
        let provable = chain[..walked].iter().all(|&(object, _)| {
            self.object_data(object).is_some_and(|data| {
                !data.has_side_descriptors() && !self.shape_is_dictionary(data.shape())
            })
        });
        if let Some(slots) = receiver_slots.filter(|_| memoized && last.is_none_or(|last| last == 0)) {
            return Some(self.record_receiver_for_in_keys(&chain, slots, provable));
        }
        let Some(last) = last else {
            return Some(self.array_of(&[]));
        };
        let mut visited = FxHashSet::default();
        let mut result: Vec<Value> = Vec::new();
        for (depth, &(object, _)) in chain[..=last].iter().enumerate() {
            let memo = if depth == last { self.shape_enumerable_slots(object) } else { None };
            match memo {
                Some(slots) => {
                    for &(atom, _) in slots.iter() {
                        if !Self::for_in_shadowed(&mut visited, atom, depth, last) {
                            result.push(self.atom_string(atom));
                        }
                    }
                }
                None => {
                    for (atom, enumerable) in self.for_in_ordinary_names(object) {
                        if Self::for_in_shadowed(&mut visited, atom, depth, last) || !enumerable {
                            continue;
                        }
                        result.push(self.atom_string(atom));
                    }
                }
            }
        }
        let keys = self.array_of(&result);
        if provable {
            self.for_in.walked.record(keys, self.heap.collection_epoch(), &chain[..=last]);
        } else {
            self.for_in.walked.forget();
        }
        Some(keys)
    }

    /// Whether `atom`, an own name of chain object `depth`, is shadowed by an earlier object of
    /// the chain. Objects before `last` record their names in `visited` for the objects after
    /// them, whatever the names' enumerability; the last object only checks.
    #[inline(always)]
    fn for_in_shadowed(visited: &mut FxHashSet<Atom>, atom: Atom, depth: usize, last: usize) -> bool {
        if depth < last {
            !visited.insert(atom) && depth > 0
        } else {
            depth > 0 && visited.contains(&atom)
        }
    }

    /// The keys of `receiver`, which alone contributes to its walk (`shape_for_in_keys`): `slots`
    /// are its enumerable keys, `chain` the walk from it to the end of the chain. The keys are
    /// recorded as the memo `ForInOwnKeys` a later loop over such a receiver reuses, and the walk
    /// as the receiver's shape (`ForInWalk`), which is all a step over its keys needs.
    fn record_receiver_for_in_keys(&mut self, chain: &[(Value, u32)], slots: EnumerableSlots, provable: bool) -> Value {
        let (shape, epoch) = (chain[0].1, self.heap.collection_epoch());
        let names: Vec<Value> = slots.iter().map(|&(atom, _)| self.atom_string(atom)).collect();
        let keys = self.array_of(&names);
        // A slot read answers a plain named key of a shape without accessors (`enumerated_key_get`).
        let accessors = self.shape_may_have_accessors(shape);
        let words: Vec<Value> = slots.iter().map(|&(atom, slot)| match u16::try_from(slot) {
            Ok(slot) if !accessors && self.plain_named_key(atom) => Value::integer(i32::from(slot)),
            _ => Value::UNDEFINED,
        }).collect();
        let slot_words = self.array_of(&words);
        // The memo holds while the chain past the receiver keeps its cell (`chain_cell_to_end`).
        let prototype = chain.get(1).map_or(Value::NULL, |&(object, _)| object);
        self.for_in.own_keys = self.chain_cell_to_end(prototype)
            .map(|cell| ForInOwnKeys { shape, epoch, keys, slot_words, slots, prototype, chain: cell, stepped: None });
        if provable {
            self.for_in.walked.record(keys, epoch, &chain[..1]);
        } else {
            self.for_in.walked.forget();
        }
        keys
    }

    /// Whether the walk recorded for the snapshot `keys` still holds over `object`: `object` is the
    /// walk's receiver, and each object of the walk has its recorded shape, no side descriptors,
    /// and the prototype link the walk followed (`ForInWalk`). Nothing else is read.
    #[inline(always)]
    fn for_in_walk_holds(&self, object: Value, keys: Value) -> bool {
        let walked = &self.for_in.walked;
        if walked.keys != Some(keys) || walked.epoch != self.heap.collection_epoch() {
            return false;
        }
        let chain = walked.chain.as_slice();
        if chain.first().is_none_or(|&(first, _)| first != object) {
            return false;
        }
        chain.iter().enumerate().all(|(index, &(walked_object, shape))| {
            self.object_data(walked_object).is_some_and(|data| {
                data.shape() == shape
                    && !data.has_side_descriptors()
                    && chain.get(index + 1).is_none_or(|&(next, _)| data.proto() == next)
            })
        })
    }

    /// `object_for_in_key_is_enumerable` for `key` of the snapshot `keys` over `object`. A key the
    /// recorded walk still holds for is an own enumerable property of `object` (`for_in_walk_holds`),
    /// so no lookup is made. Any other step takes the walk, which skips a key deleted or made
    /// non-enumerable since the snapshot.
    #[inline]
    fn for_in_key_is_enumerable(&mut self, p: &ResidualProgram, object: Value, keys: Value, key: Value) -> Result<bool, JsError> {
        if self.for_in_walk_holds(object, keys) {
            return Ok(true);
        }
        self.object_for_in_key_is_enumerable(p, object, key)
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
        // SAFETY: the snapshot recorded this slot of `shape`, the receiver's shape.
        Some(unsafe { self.heap.property_get_unchecked(data, slot as usize) })
    }

    /// `base[key]` of a string `key` that names no array index, where the shapes alone answer:
    /// the slot of the key a for-in just stepped to (`enumerated_key_get`), or a read by its
    /// atom as a named read is, through the (shape, key) proofs (`plain_named_load`, which
    /// refuses index atoms). `None` for any other key, or a read those decline.
    #[inline(always)]
    pub(in crate::vm) fn plain_string_keyed_get(&mut self, base: Value, key: Value) -> Option<Value> {
        if let Some(value) = self.enumerated_key_get(base, key) {
            return Some(value);
        }
        let atom = self.lookup_string_atom(key)?;
        self.plain_named_load(base, atom)
    }
}

impl<H: Host> Vm<H> {
    /// The array of for-in `keys` with `prototype`.
    pub(super) fn alloc_for_in_key_array(&mut self, keys: crate::heap::ArrayElements, prototype: Value) -> Value {
        self.heap.alloc_array(crate::heap::ArrayObject::new(Self::empty_object(prototype), keys))
    }
}
