//! `JSON.stringify`'s one writer (`Vm::json_write_member`): SerializeJSONProperty of each
//! value, which writes plain data in place (primitives, dense arrays, and ordinary objects by
//! the member plan of their (shape, prototype), `JsonMember`) and takes the specification's
//! steps for any other value (`Vm::json_serialize_property`). The steady call
//! (`Vm::json_stringify_steady`) runs it with no program, stopping at the first value that
//! needs those steps; the native (`Vm::json_stringify`) tries that call first, and runs the
//! one writer with its replacer and gap when it declines. The plans are kept between calls
//! (`JsonPlainCache`).

use super::*;

/// The most text capacity `Vm::json_plain` keeps between calls: enough for a multi-megabyte
/// payload's serialization, bounded so one huge result does not stay resident.
const JSON_RETAINED_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

/// The nesting the writer recurses through without a stack guard; each deeper container
/// takes one (`Vm::json_stack_guard`). Request and response bodies nest a few levels.
const JSON_PLAIN_DEPTH: usize = 64;

/// The plans `JsonPlainWrite::recent_plans` holds.
const JSON_RECENT_PLANS: usize = 8;

/// The containers being written, inline up to this nesting.
const JSON_PLAIN_INLINE_ANCESTORS: usize = 16;

/// The state of one plain stringification: no guest code runs during it, so what a shape
/// and prototype decide (lacking `toJSON`, an object's members) holds for its whole run.
struct JsonPlainWrite {
    to_json: Atom,
    ancestors: smallvec::SmallVec<[Value; JSON_PLAIN_INLINE_ANCESTORS]>,
    array_lacks_to_json: Option<(u32, Value)>,
    output: JsonText,
    /// The members of each ordinary object's (shape, prototype) written so far, or `None`
    /// when objects of it take the general serializer.
    object_plans: rustc_hash::FxHashMap<(u32, Value), Option<std::rc::Rc<[JsonMember]>>>,
    /// Recently used plans by (shape, prototype), one per shape's low bits: a payload's few
    /// kinds of object recur, nested inside one another, which this answers without hashing.
    recent_plans: [Option<((u32, Value), std::rc::Rc<[JsonMember]>)>; JSON_RECENT_PLANS],
    /// The `"key":` text of every planned member: an object's keys recur across the objects
    /// of a payload, so each shape's are quoted once.
    key_text: JsonText,
    /// The prototypes whose chains this run's new plans read (lacking `toJSON`), each once:
    /// the run's end holds the plans under their subjects (`Vm::hold_json_plans`).
    chains: Vec<Value>,
    /// The subjects of every prototype a kept plan's chain holds, each once.
    subjects: Vec<u32>,
}

/// One enumerable data member of a planned object: its slot and its `,"key":` text's range in
/// `JsonPlainWrite::key_text`, whose first byte, the comma, a first written member skips.
#[derive(Clone, Copy)]
struct JsonMember {
    slot: u32,
    key: (u32, u32),
}

/// What plain stringifications keep between calls (`Vm::json_plain`): the text buffer, and
/// the member plans of the (shape, prototype) pairs met, as V8 keeps a map's "fast
/// serializable" bit. A plan is a fact of a shape and a prototype chain, so it holds while the
/// collection epoch (shape ids, prototype records) stays where it was when it was made and
/// the cell of the prototypes the plans' chains hold does (a prototype gaining `toJSON` or an
/// accessor retires it); a change to a prototype no plan's chain holds keeps the plans.
#[derive(Default)]
pub(crate) struct JsonPlainCache {
    collection_epoch: u64,
    /// The cell of `JsonPlainWrite::subjects` (`Dependencies::subjects_cell`); `None` before
    /// any plan is kept.
    cell: Option<crate::heap::ChainCell>,
    /// Boxed so a call moves the state out and back as one pointer.
    write: Option<Box<JsonPlainWrite>>,
}

/// Bytes of quoted keys past which the plans are dropped rather than kept: a payload of many
/// distinct shapes (a data dump) would otherwise keep every key resident.
const JSON_RETAINED_KEY_BYTES: usize = 1 << 16;

impl JsonPlainWrite {
    /// Forgets every plan, keeping the atom and the buffers.
    fn forget_plans(&mut self) {
        self.array_lacks_to_json = None;
        self.object_plans.clear();
        self.recent_plans = Default::default();
        self.key_text.clear();
        self.chains.clear();
        self.subjects.clear();
    }

    /// Notes that a new plan read the chain from `prototype`.
    fn note_chain(&mut self, prototype: Value) {
        if !self.chains.contains(&prototype) {
            self.chains.push(prototype);
        }
    }

    fn new(to_json: Atom) -> Self {
        Self {
            to_json,
            ancestors: Default::default(),
            array_lacks_to_json: None,
            output: JsonText::new(),
            object_plans: Default::default(),
            recent_plans: Default::default(),
            key_text: JsonText::new(),
            chains: Vec::new(),
            subjects: Vec::new(),
        }
    }
}

/// `JSON.stringify(value, replacer, space)`'s argument positions.
const JSON_REPLACER_ARGUMENT: usize = 1;
const JSON_SPACE_ARGUMENT: usize = 2;

impl<H: Host> Vm<H> {
    /// `JSON.stringify(value)`'s steady call, when it passes no replacer and no gap (arguments
    /// past those two are not read): the one writer with no program (`json_run`), which
    /// answers where the value is plain data; `None` leaves the call to the native, having
    /// observed nothing.
    #[inline]
    pub(in crate::vm) fn json_stringify_steady(&mut self, args: &[Value]) -> Option<Value> {
        let unset = |index: usize| args.get(index).is_none_or(|value| value.is_undefined());
        if !(unset(JSON_REPLACER_ARGUMENT) && unset(JSON_SPACE_ARGUMENT)) {
            return None;
        }
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        self.json_run(None, value, JsonOptions::default()).ok()
    }

    /// JSON.stringify(value) by the plain writer: the string, or `undefined` for an omitted
    /// value; `None` when the value needs the general steps, having observed nothing.
    pub(super) fn json_stringify_plain(&mut self, value: Value) -> Option<Value> {
        let collection_epoch = self.heap.collection_epoch();
        let held = self.json_plain.collection_epoch == collection_epoch && self.json_plain.cell.is_some_and(crate::heap::ChainCell::holds);
        let mut plain = match self.json_plain.write.take() {
            Some(plain) if held => plain,
            Some(mut plain) => {
                plain.forget_plans();
                plain
            }
        };
        let written = self.json_write_plain(value, &mut plain);
        let text = written
            .filter(|&written| written)
            .map(|_| JsString::from_str(json_text_str(&plain.output)));
        plain.output.clear();
        plain.ancestors.clear();
        if plain.output.capacity() > JSON_RETAINED_OUTPUT_BYTES {
            plain.output = JsonText::new();
        }
        if plain.key_text.len() > JSON_RETAINED_KEY_BYTES {
            plain.forget_plans();
        }
        let cell = self.hold_json_plans(&mut plain, held.then_some(self.json_plain.cell).flatten());
        self.json_plain = JsonPlainCache { collection_epoch, cell, write: Some(plain) };
        match (written, text) {
            (Some(_), Some(text)) => Some(self.heap.alloc_string(text)),
            (Some(_), None) => Some(Value::UNDEFINED),
            (None, _) => None,
        }
    }

    /// The cell the plans `plain` keeps hold under: `held`, the one they held under at the
    /// run's start, while no new plan read a prototype it does not cover; else a cell of
    /// every prototype their chains hold, each marked a prototype first. The run ran no guest
    /// code, so the chains its plans read are as they were. `None`, with the plans forgotten,
    /// where a link has no subject.
    fn hold_json_plans(&mut self, plain: &mut JsonPlainWrite, held: Option<crate::heap::ChainCell>) -> Option<crate::heap::ChainCell> {
        let mut grew = held.is_none();
        for prototype in std::mem::take(&mut plain.chains) {
            let Some(subjects) = self.chain_subjects_to_end(prototype) else {
                plain.forget_plans();
                return None;
            };
            for subject in subjects {
                if !plain.subjects.contains(&subject) {
                    plain.subjects.push(subject);
                    grew = true;
                }
            }
        }
        if grew { Some(self.heap.dependencies().subjects_cell(&plain.subjects)) } else { held }
    }

    /// The subjects of the whole chain from `prototype` (`fact_chain_subjects`), each marked
    /// a prototype first; `None` past `u8::MAX` links or where one has no subject.
    fn chain_subjects_to_end(&mut self, prototype: Value) -> Option<smallvec::SmallVec<[u32; crate::heap::INHERITED_FIELD_CACHE_MAX_DEPTH]>> {
        let mut depth = 0u8;
        let mut link = prototype;
        while !link.is_null() {
            link = self.heap.object(link)?.proto();
            depth = depth.checked_add(1)?;
        }
        self.fact_chain_subjects(prototype, depth)
    }

    /// SerializeJSONProperty of `value` with no replacer and no gap, for plain data whose
    /// serialization runs no guest code: primitives, dense hole-free arrays and ordinary
    /// objects of non-dictionary shapes with only data members, none of which a `toJSON`
    /// reaches. `Some(false)` is an omitted value. `None` leaves the whole stringification to
    /// the general steps, having observed nothing: a cycle, getter, `toJSON`, function,
    /// BigInt, boxed primitive, index key, exotic object or nesting beyond
    /// `JSON_PLAIN_DEPTH`.
    fn json_write_plain(&self, value: Value, plain: &mut JsonPlainWrite) -> Option<bool> {
        if let Some(written) = self.json_write_plain_primitive(value, &mut plain.output) {
            return Some(written);
        }
        if writer.key_text.len() > JSON_RETAINED_KEY_BYTES {
            writer.forget_plans();
        }
        self.json_plain.write = Some(writer);
    }

    /// SerializeJSONProperty of the root value under the key "" of its wrapper object, which
    /// is made only for a replacer: only a replacer receives it (as `this`).
    fn json_write_root(&mut self, p: Option<&ResidualProgram>, value: Value, w: &mut JsonWriter) -> JsonStep {
        let key = JsonKey::Name(w.empty);
        if w.options.replacer.is_none() {
            return self.json_write_member(p, Value::UNDEFINED, key, Some(value), w);
        }
        let holder = self.json_object();
        self.with_call_roots([holder], |vm| {
            vm.set_property(holder, w.empty, value)?;
            // Making the wrapper may have collected.
            w.after_guest_code(vm.json_epochs());
            vm.json_write_member(p, holder, key, Some(value), w)
        })
    }

    /// SerializeJSONProperty(holder, key) of a member whose value `value` was read without
    /// guest code, or `None` for one [[Get]] must read. With no replacer, a primitive, a plain
    /// container or an object no `toJSON` reaches is written as it is: the property steps
    /// would call nothing. Any other member takes them, or stops the steady call.
    fn json_write_member(
        &mut self,
        p: Option<&ResidualProgram>,
        holder: Value,
        key: JsonKey,
        value: Option<Value>,
        w: &mut JsonWriter,
    ) -> JsonStep {
        if let Some(value) = value.filter(|_| w.options.replacer.is_none()) {
            if let Some(written) = self.json_write_primitive(value, &mut w.output) {
                return Ok(written);
            }
            if let Some(written) = self.json_write_plain_container(p, value, w)? {
                return Ok(written);
            }
            if let Some(p) = p
                && self.is_object_like(value)
                && self.json_lacks_to_json(value, w.to_json)
            {
                return self.json_serialize_value(p, value, w);
            }
        }
        self.json_serialize_property(p.ok_or(JsonStop::Plain)?, holder, key, value, w)
    }

    /// SerializeJSONValue of a primitive: whether it was written (`false`: omitted, for
    /// undefined and symbols), or `None` for an object or BigInt.
    pub(super) fn json_write_primitive(&self, value: Value, output: &mut JsonText) -> Option<bool> {
        if value.is_undefined() || value.is_deleted() {
            return Some(false);
        }
        if value.is_null() {
            output.extend_from_slice(b"null");
        } else if let Some(value) = value.as_bool() {
            output.extend_from_slice(if value { b"true" } else { b"false" });
        } else if let Some(integer) = value.as_int() {
            write_json_integer(integer, output);
        } else if let Some(number) = value.as_number() {
            if number.is_finite() {
                crate::number_to_string::write(number, output);
            } else {
                output.extend_from_slice(b"null");
            }
        } else if let Some(text) = self.heap.string(value) {
            write_json_string(text, output);
        } else {
            return self.heap.is_symbol(value).then_some(false);
        }
        Some(true)
    }

    /// SerializeJSONValue of a container read as it is, none of whose own steps runs guest
    /// code: a dense array (`json_plain_array`) or an ordinary object of a member plan
    /// (`json_plain_plan`), written whole (`Some(true)`). `None` for any other value.
    pub(super) fn json_write_plain_container(
        &mut self,
        p: Option<&ResidualProgram>,
        value: Value,
        w: &mut JsonWriter,
    ) -> Result<Option<bool>, JsonStop> {
        match self.json_plain_array(value, w) {
            Some(true) => return self.json_write_array(p, value, w).map(|()| Some(true)),
            Some(false) => return Ok(None),
            None => {}
        }
        let key = (array.shape(), array.proto());
        if plain.array_lacks_to_json != Some(key) {
            if !self.json_lacks_to_json(value, plain.to_json) {
                return false;
            }
            plain.array_lacks_to_json = Some(key);
            plain.note_chain(key.1);
        }
    }

    /// Whether the array `value` is written as it is: its elements held densely
    /// (`array_search_elements`), no side-table descriptors, and no `toJSON` along its chain,
    /// decided once per (shape, prototype) of a run's arrays. `None` for a value that is no
    /// array.
    fn json_plain_array(&self, value: Value, w: &mut JsonWriter) -> Option<bool> {
        let array = self.heap.array(value)?;
        if array.has_side_descriptors() || self.array_search_elements(value).is_none() {
            return Some(false);
        }
        let key = (array.shape(), array.proto());
        if w.array_lacks_to_json != Some(key) {
            if !self.json_lacks_to_json(value, w.to_json) {
                return Some(false);
            }
            w.array_lacks_to_json = Some(key);
        }
        Some(true)
    }

    /// SerializeJSONArray of `array`: its length read once, then each element, read from the
    /// dense backing where it is present (`json_element`), else by [[Get]].
    pub(super) fn json_write_array(
        &mut self,
        p: Option<&ResidualProgram>,
        array: Value,
        w: &mut JsonWriter,
    ) -> Result<(), JsonStop> {
        self.json_container(p, array, w, |vm, w| {
            let dense = vm.array_search_elements(array).map(|elements| elements.len());
            let length = match dense {
                Some(length) => length,
                None => vm.json_array_length(p.ok_or(JsonStop::Plain)?, array, w)?,
            };
            let proved = dense.map(|_| w.steps);
            w.output.push(b'[');
            for index in 0..length {
                w.begin_member(index);
                let element = vm.json_element(array, index, proved == Some(w.steps));
                if !vm.json_write_member(p, array, JsonKey::Index(index), element, w)? {
                    w.output.extend_from_slice(b"null");
                }
            }
            w.end_container(length, b']');
            Ok(())
        })
    }

    /// LengthOfArrayLike of an array without a dense backing (a Proxy's, or one of sparse
    /// storage or indexed descriptors), which may run guest code.
    fn json_array_length(&mut self, p: &ResidualProgram, array: Value, w: &mut JsonWriter) -> Result<usize, JsonStop> {
        let length = self.array_like_length(p, array)?;
        w.after_guest_code(self.json_epochs());
        Ok(length)
    }

    /// Element `index` of `array` where reading it runs no guest code: a present element of
    /// its dense backing, which holds as proved while no guest code has run (`proved`).
    fn json_element(&self, array: Value, index: usize, proved: bool) -> Option<Value> {
        let elements = if proved {
            self.heap.array(array).map(|array| array.elements())
        } else {
            self.array_search_elements(array)
        };
        elements?.get(index).filter(|element| !element.is_deleted())
    }

    /// SerializeJSONObject of `object`: the members of its plan, or else the keys the
    /// specification's steps give it (`json_member_keys`), each written by `json_write_member`.
    pub(super) fn json_write_object(
        &mut self,
        p: Option<&ResidualProgram>,
        object: Value,
        plan: Option<std::rc::Rc<[JsonMember]>>,
        w: &mut JsonWriter,
    ) -> Result<(), JsonStop> {
        self.json_container(p, object, w, |vm, w| {
            let members = match plan {
                Some(plan) => JsonMembers::Planned { plan, steps: w.steps },
                None => JsonMembers::Keys(vm.json_member_keys(p, object, w)?),
            };
            vm.json_write_members(p, object, &members, w)
        })
    }

    fn json_write_members(
        &mut self,
        p: Option<&ResidualProgram>,
        object: Value,
        members: &JsonMembers,
        w: &mut JsonWriter,
    ) -> Result<(), JsonStop> {
        w.output.push(b'{');
        let mut written = 0;
        for index in 0..members.len() {
            // The member is written before its value is known; an omitted value takes it back.
            let start = w.output.len();
            let (key, value) = self.json_begin_member(object, members, index, written, w);
            if self.json_write_member(p, object, JsonKey::Name(key), value, w)? {
                written += 1;
            } else {
                w.output.truncate(start);
            }
        }
        w.end_container(written, b'}');
        Ok(())
    }

    /// Writes member `index`'s separator and key (`written` members precede it), and reads
    /// its value where that runs no guest code: from its planned slot while no guest code has
    /// run since the plan was read, else by key along plain shapes (`plain_named_get`). `None`
    /// leaves the read to [[Get]].
    fn json_begin_member(
        &self,
        object: Value,
        members: &JsonMembers,
        index: usize,
        written: usize,
        w: &mut JsonWriter,
    ) -> (Atom, Option<Value>) {
        match members {
            JsonMembers::Planned { plan, steps } => {
                let member = plan[index];
                w.begin_planned_member(member, written);
                let value = if *steps == w.steps {
                    self.json_planned_value(object, member.slot)
                } else {
                    self.plain_named_get(object, member.atom)
                };
                (member.atom, value)
            }
            JsonMembers::Keys(keys) => {
                let key = keys[index];
                w.begin_member(written);
                self.write_json_key(key, &mut w.output);
                w.write_colon();
                (key, self.plain_named_get(object, key))
            }
        }
    }

    /// The value in `slot` of a planned object, read while no guest code has run since its
    /// plan was read; `None` where the slot holds none.
    fn json_planned_value(&self, object: Value, slot: u32) -> Option<Value> {
        let data = self.heap.object_record(object)?;
        // SAFETY: the plan's slots are its shape's, which the object keeps while no guest code
        // runs.
        unsafe { self.heap.property_get_shaped(data, slot as usize) }.filter(|value| !value.is_deleted())
    }

    /// Writes the container `value` by `body`: SerializeJSONObject's and SerializeJSONArray's
    /// cycle test, and a stack guard past `JSON_PLAIN_DEPTH`. Where guest code may run (a
    /// program), the container is rooted while its members are written.
    fn json_container(
        &mut self,
        p: Option<&ResidualProgram>,
        value: Value,
        w: &mut JsonWriter,
        body: impl FnOnce(&mut Self, &mut JsonWriter) -> Result<(), JsonStop>,
    ) -> Result<(), JsonStop> {
        // Every ancestor and `value` is an object, whose SameValue is its identity.
        if w.ancestors.contains(&value) {
            return Err(match p {
                Some(p) => self.type_error(p, JSON_CIRCULAR_MESSAGE.into()).into(),
                None => JsonStop::Plain,
            });
        }
        let _stack = self.json_stack_guard(p, w.ancestors.len())?;
        w.ancestors.push(value);
        let result = match p {
            Some(_) => self.with_call_roots([value], |vm| body(vm, w)),
            None => body(self, w),
        };
        w.ancestors.pop();
        result
    }

    /// The stack guard of a container `depth` levels deep, past `JSON_PLAIN_DEPTH`; its
    /// exhaustion raises the RangeError, or stops the steady call.
    fn json_stack_guard(
        &mut self,
        p: Option<&ResidualProgram>,
        depth: usize,
    ) -> Result<Option<crate::stack::StackGuard>, JsonStop> {
        if depth < JSON_PLAIN_DEPTH {
            return Ok(None);
        }
        match crate::stack::StackGuard::enter() {
            Ok(guard) => Ok(Some(guard)),
            Err(crate::stack::StackExhausted) if p.is_some() => Err(self.stack_exhaustion_error().into()),
            Err(crate::stack::StackExhausted) => Err(JsonStop::Plain),
        }
    }

    /// The member plan of `value`, an ordinary object of non-dictionary shape whose own
    /// record holds no side-table descriptors, raw JSON or boxed primitive, when no replacer
    /// property list names its keys (`json_plain_members`).
    fn json_plain_plan(&self, value: Value, w: &mut JsonWriter) -> Option<std::rc::Rc<[JsonMember]>> {
        if w.options.property_list.is_some() {
            return None;
        }
        let data = self.heap.object_record(value)?;
        // Shape facts decide the plan; these per-object facts do not.
        if data.is_raw_json() || data.has_side_descriptors() || self.boxed_primitive_data(value).is_some() {
            return None;
        }
        self.json_plain_members(value, data, w)
    }

    /// The members of `value`'s (shape, prototype), derived on the first object of it: its
    /// enumerable string-keyed data properties in order, when it lacks `toJSON` and no
    /// property may be an accessor (whose kind a shape alone does not decide).
    fn json_plain_members(
        &self,
        value: Value,
        data: &crate::heap::Object,
        w: &mut JsonWriter,
    ) -> Option<std::rc::Rc<[JsonMember]>> {
        let key = (data.shape(), data.proto());
        let recent = key.0 as usize % JSON_RECENT_PLANS;
        if let Some((last, plan)) = &w.recent_plans[recent]
            && *last == key
        {
            return Some(plan.clone());
        }
        if let Some(plan) = w.object_plans.get(&key) {
            if let Some(plan) = plan {
                w.recent_plans[recent] = Some((key, plan.clone()));
            }
            return plan.clone();
        }
        let plan = (!self.shape_may_have_accessors(data.shape()) && self.json_lacks_to_json(value, w.to_json))
            .then(|| self.shape_enumerable_slots(value))
            .flatten()
            .and_then(|slots| {
                slots
                    .iter()
                    .map(|&(atom, slot)| {
                        self.plain_named_key(atom).then(|| {
                            let start = w.key_text.len() as u32;
                            w.key_text.push(b',');
                            self.write_json_key(atom, &mut w.key_text);
                            w.key_text.push(b':');
                            JsonMember { atom, slot, key: (start, w.key_text.len() as u32) }
                        })
                    })
                })
                .collect::<Option<std::rc::Rc<[JsonMember]>>>()
        });
        plain.object_plans.insert(key, plan.clone());
        plain.note_chain(key.1);
        plan
    }
}

#[cfg(test)]
mod tests {
    use crate::test_host::TestHost;
    use crate::vm::Vm;
    use crate::Engine;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn json_stringify_plans_kept_between_calls_follow_prototype_changes() {
        // The writer keeps its (shape, prototype) plans between calls; a prototype that gains or
        // loses `toJSON` afterwards, on any link of the chain, changes the next call's result.
        let source = r#"
const out = [];
class P { constructor() { this.a = 1; this.b = [2]; } }
const p = new P(), q = { c: p };
out.push(JSON.stringify(q), JSON.stringify(q));
P.prototype.toJSON = function () { return 'P'; }; out.push(JSON.stringify(q));
delete P.prototype.toJSON; out.push(JSON.stringify(q));
Object.prototype.toJSON = function () { return 'O'; }; out.push(JSON.stringify(q)); delete Object.prototype.toJSON;
out.push(JSON.stringify([q, [1]]));
Array.prototype.toJSON = function () { return 'A'; }; out.push(JSON.stringify([q, [1]])); delete Array.prototype.toJSON;
p.toJSON = () => 'own'; out.push(JSON.stringify(q)); delete p.toJSON; out.push(JSON.stringify(q));
Object.setPrototypeOf(p, { toJSON() { return 'swapped'; } }); out.push(JSON.stringify(q));
print(JSON.stringify(out));
"#;
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(output.clone()));
        vm.execute(&Engine::specialize(source, "json_stringify_plans_kept_between_calls.js").unwrap()).unwrap();
        assert_eq!(output.borrow().as_slice(), [r#"["{\"c\":{\"a\":1,\"b\":[2]}}","{\"c\":{\"a\":1,\"b\":[2]}}","{\"c\":\"P\"}","{\"c\":{\"a\":1,\"b\":[2]}}","\"O\"","[{\"c\":{\"a\":1,\"b\":[2]}},[1]]","\"A\"","{\"c\":\"own\"}","{\"c\":{\"a\":1,\"b\":[2]}}","{\"c\":\"swapped\"}"]"#]);
    }
}
