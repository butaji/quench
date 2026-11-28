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

/// Bytes of quoted keys past which the plans are dropped rather than kept: a payload of many
/// distinct shapes (a data dump) would otherwise keep every key resident.
const JSON_RETAINED_KEY_BYTES: usize = 1 << 16;

/// SerializeJSONObject's and SerializeJSONArray's TypeError for a value already being written.
const JSON_CIRCULAR_MESSAGE: &str = "Converting circular structure to JSON";

impl Vm {
    /// `JSON.stringify(value)`'s steady call, when it passes no replacer and no gap (arguments
    /// past those two are not read): the one writer with no program (`json_run`), which
    /// answers where the value is plain data; `None` leaves the call to the native, having
    /// observed nothing.
    #[inline]
    pub fn json_stringify_steady(&mut self, args: &[Value]) -> Option<Value> {
        let unset = |index: usize| args.get(index).is_none_or(|value| value.is_undefined());
        if !(unset(JSON_REPLACER_ARGUMENT) && unset(JSON_SPACE_ARGUMENT)) {
            return None;
        }
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        self.json_run(None, value, JsonOptions::default()).ok()
    }

    /// The serialization of `value` under `options` by the one writer: the string, or
    /// `undefined` for an omitted value. With no program (`p`), it stops at the first value
    /// that needs the specification's steps.
    pub(super) fn json_run(
        &mut self,
        p: Option<&ResidualProgram>,
        value: Value,
        options: JsonOptions,
    ) -> Result<Value, JsonStop> {
        let mut writer = self.json_writer(options);
        let text = self
            .json_write_root(p, value, &mut writer)
            .map(|written| written.then(|| JsString::from_str(json_text_str(&writer.output))));
        self.json_keep_writer(writer);
        Ok(match text? {
            Some(text) => self.heap.alloc_string(text),
            None => Value::UNDEFINED,
        })
    }

    /// The collection epoch a plan is proved under (`JsonPlainCache`): a collection may reuse a
    /// freed record's id. A prototype's layout is not in it: each proof holds the chain cell of
    /// the prototypes it read (`json_chain_cell`), which a layout change of any of them retires.
    pub(super) fn json_epoch(&self) -> u64 {
        self.heap.collection_epoch()
    }

    /// The validity cell of the prototype chain from `prototype` to its end, which a proof about
    /// the prototypes `json_lacks_to_json` and the member plans read holds on (`ChainCell`).
    /// `None` where the chain cannot be named: the proof is then not kept.
    fn json_chain_cell(&mut self, prototype: Value) -> Option<crate::heap::ChainCell> {
        self.chain_cell_to_end(prototype)
    }

    /// The kept writer (`JsonPlainCache`), or a new one, set up for a run under `options`.
    fn json_writer(&mut self, options: JsonOptions) -> Box<JsonWriter> {
        let epoch = self.json_epoch();
        let mut writer = match self.json_plain.write.take() {
            Some(writer) => writer,
            None => {
                let to_json = self.intern_atom("toJSON");
                let empty = self.intern_atom("");
                Box::new(JsonWriter::new(to_json, empty, epoch))
            }
        };
        writer.prove(epoch);
        writer.options = options;
        writer
    }

    /// Keeps `writer` for the next run, its text and options dropped, bounded so one huge
    /// result or a payload of many shapes does not stay resident.
    fn json_keep_writer(&mut self, mut writer: Box<JsonWriter>) {
        writer.output.clear();
        writer.ancestors.clear();
        writer.options = JsonOptions::default();
        if writer.output.capacity() > JSON_RETAINED_OUTPUT_BYTES {
            writer.output = JsonText::new();
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
            w.after_guest_code(vm.json_epoch());
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
        match self.json_plain_plan(value, w) {
            Some(plan) => self.json_write_object(p, value, Some(plan), w).map(|()| Some(true)),
            None => Ok(None),
        }
    }

    /// Whether the array `value` is written as it is: its elements held densely
    /// (`array_search_elements`), no side-table descriptors, and no `toJSON` along its chain,
    /// decided once per (shape, prototype) of a run's arrays and held while the chain cell of
    /// the prototype holds. `None` for a value that is no array.
    fn json_plain_array(&mut self, value: Value, w: &mut JsonWriter) -> Option<bool> {
        let array = self.heap.array(value)?;
        if array.has_side_descriptors() || self.array_search_elements(value).is_none() {
            return Some(false);
        }
        let key = (array.shape(), array.proto());
        if w.array_lacks_to_json.is_some_and(|(proved, cell)| proved == key && cell.holds()) {
            return Some(true);
        }
        if !self.json_lacks_to_json(value, w.to_json) {
            return Some(false);
        }
        w.array_lacks_to_json = self.json_chain_cell(key.1).map(|cell| (key, cell));
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
        w.after_guest_code(self.json_epoch());
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
        let data = self.heap.direct_object_view(object)?;
        // SAFETY: the plan's slots are its shape's, which the object keeps while no guest code
        // runs.
        unsafe { self.heap.property_get_shaped_view(data, slot as usize) }.filter(|value| !value.is_deleted())
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
    fn json_plain_plan(&mut self, value: Value, w: &mut JsonWriter) -> Option<std::rc::Rc<[JsonMember]>> {
        if w.options.property_list.is_some() {
            return None;
        }
        let data = self.heap.direct_object_view(value)?;
        // Shape facts decide the plan; these per-object facts do not.
        if data.is_raw_json() || data.has_side_descriptors() || self.boxed_primitive_data(value).is_some() {
            return None;
        }
        let key = (data.shape(), data.proto());
        self.json_plain_members(value, key, w)
    }

    /// The members of `value`'s (shape, prototype), derived on the first object of it: its
    /// enumerable string-keyed data properties in order, when it lacks `toJSON` and no
    /// property may be an accessor (whose kind a shape alone does not decide). The plan, or its
    /// refusal, is held while the chain cell of the prototype holds (`JsonPlanProof`); where
    /// that cell cannot be made, it is not kept.
    fn json_plain_members(
        &mut self,
        value: Value,
        key: (u32, Value),
        w: &mut JsonWriter,
    ) -> Option<std::rc::Rc<[JsonMember]>> {
        let shape = key.0 as usize;
        if let Some(Some((prototype, proof))) = w.plans.get(shape)
            && *prototype == key.1
            && proof.cell.holds()
        {
            return proof.plan.clone();
        }
        // `json_plain_plan` decided that `value` is not a boxed primitive.
        let plan = (!self.shape_may_have_accessors(key.0) && self.json_chain_lacks_to_json(value, w.to_json))
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
                    .collect::<Option<std::rc::Rc<[JsonMember]>>>()
            });
        if let Some(cell) = self.json_chain_cell(key.1) {
            if w.plans.len() <= shape {
                w.plans.resize(shape + 1, None);
            }
            w.plans[shape] = Some((key.1, JsonPlanProof { cell, plan: plan.clone() }));
        }
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
