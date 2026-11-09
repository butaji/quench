//! `JSON.stringify` with no replacer and no gap, for plain data whose serialization runs no
//! guest code (`Vm::json_stringify_plain`). The writer reads each object's members from a
//! plan of its (shape, prototype) and keeps the plans between calls (`JsonPlainCache`).

use super::*;

/// The most text capacity `Vm::json_plain` keeps between calls: enough for a multi-megabyte
/// payload's serialization, bounded so one huge result does not stay resident.
const JSON_RETAINED_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

/// The nesting the plain writer (`Vm::json_write_plain`) recurses through without counting
/// stack; deeper data takes the general steps, which do. Request and response bodies nest a
/// few levels.
const JSON_PLAIN_DEPTH: usize = 64;

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
    /// The `"key":` text of every planned member: an object's keys recur across the objects
    /// of a payload, so each shape's are quoted once.
    key_text: JsonText,
}

/// One enumerable data member of a planned object: its slot and its `"key":` text's range in
/// `JsonPlainWrite::key_text`.
#[derive(Clone, Copy)]
struct JsonMember {
    slot: u32,
    key: (u32, u32),
}

/// What plain stringifications keep between calls (`Vm::json_plain`): the text buffer, and
/// the member plans of the (shape, prototype) pairs met, as V8 keeps a map's "fast
/// serializable" bit. A plan is a fact of a shape and a prototype chain, so it holds while the
/// collection epoch (shape ids, prototype records) and the prototype layout epoch (a prototype
/// gaining `toJSON` or an accessor) both stay where they were when it was made.
#[derive(Default)]
pub(crate) struct JsonPlainCache {
    epochs: (u64, u64),
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
        self.key_text.clear();
    }

    fn new(to_json: Atom) -> Self {
        Self {
            to_json,
            ancestors: Default::default(),
            array_lacks_to_json: None,
            output: JsonText::new(),
            object_plans: Default::default(),
            key_text: JsonText::new(),
        }
    }
}

impl<H: Host> Vm<H> {
    /// JSON.stringify(value) by the plain writer: the string, or `undefined` for an omitted
    /// value; `None` when the value needs the general steps, having observed nothing.
    pub(super) fn json_stringify_plain(&mut self, value: Value) -> Option<Value> {
        let epochs = (self.heap.collection_epoch(), crate::heap::prototype_layout_epoch());
        let mut plain = match self.json_plain.write.take() {
            Some(plain) if self.json_plain.epochs == epochs => plain,
            Some(mut plain) => {
                plain.forget_plans();
                plain
            }
            None => Box::new(JsonPlainWrite::new(self.intern_atom("toJSON"))),
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
        self.json_plain = JsonPlainCache { epochs, write: Some(plain) };
        match (written, text) {
            (Some(_), Some(text)) => Some(self.heap.alloc_string(text)),
            (Some(_), None) => Some(Value::UNDEFINED),
            (None, _) => None,
        }
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
        if plain.ancestors.len() == JSON_PLAIN_DEPTH || plain.ancestors.contains(&value) {
            return None;
        }
        plain.ancestors.push(value);
        let written = if let Some(array) = self.heap.array(value) {
            self.json_write_plain_array(value, array, plain)
        } else {
            self.json_write_plain_object(value, plain)
        };
        plain.ancestors.pop();
        written.then_some(true)
    }

    /// `json_write_plain` of a primitive: whether it was written (`false`: omitted), or
    /// `None` for an object.
    fn json_write_plain_primitive(&self, value: Value, output: &mut JsonText) -> Option<bool> {
        if value.is_undefined() {
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
            return matches!(self.heap.get(value), Some(Cell::Symbol(_))).then_some(false);
        }
        Some(true)
    }

    fn json_write_plain_array(&self, value: Value, array: &crate::heap::ArrayObject, plain: &mut JsonPlainWrite) -> bool {
        if array.has_side_descriptors() {
            return false;
        }
        let key = (array.shape(), array.proto());
        if plain.array_lacks_to_json != Some(key) {
            if !self.json_lacks_to_json(value, plain.to_json) {
                return false;
            }
            plain.array_lacks_to_json = Some(key);
        }
        let Some(elements) = self.present_dense_elements(value, 0..array.elements().len()) else {
            return false;
        };
        plain.output.push(b'[');
        for (index, element) in elements.iter().enumerate() {
            if index != 0 {
                plain.output.push(b',');
            }
            match self.json_write_plain(*element, plain) {
                Some(true) => {}
                Some(false) => plain.output.extend_from_slice(b"null"),
                None => return false,
            }
        }
        plain.output.push(b']');
        true
    }

    fn json_write_plain_object(&self, value: Value, plain: &mut JsonPlainWrite) -> bool {
        let Some(data) = self.heap.object_record(value) else {
            return false;
        };
        // Shape facts decide the plan; these per-object facts do not.
        if data.is_raw_json() || data.has_side_descriptors() || self.boxed_primitive_data(value).is_some() {
            return false;
        }
        let Some(members) = self.json_plain_members(value, data, plain) else {
            return false;
        };
        plain.output.push(b'{');
        let mut written = false;
        for member in members.iter() {
            let Some(member_value) = self.heap.property_get(data, member.slot as usize) else {
                return false;
            };
            // The member is written before its value is known; an omitted value takes it back.
            let start = plain.output.len();
            if written {
                plain.output.push(b',');
            }
            let (key_start, key_end) = member.key;
            plain.output.extend_from_slice(&plain.key_text[key_start as usize..key_end as usize]);
            match self.json_write_plain(member_value, plain) {
                Some(true) => written = true,
                Some(false) => plain.output.truncate(start),
                None => return false,
            }
        }
        plain.output.push(b'}');
        true
    }

    /// The members of `value`'s (shape, prototype), derived on the first object of it: its
    /// enumerable string-keyed data properties in order, when it lacks `toJSON` and no
    /// property may be an accessor (whose kind a shape alone does not decide).
    fn json_plain_members(
        &self,
        value: Value,
        data: &crate::heap::Object,
        plain: &mut JsonPlainWrite,
    ) -> Option<std::rc::Rc<[JsonMember]>> {
        let key = (data.shape(), data.proto());
        if let Some(plan) = plain.object_plans.get(&key) {
            return plan.clone();
        }
        let plan = (!self.shape_may_have_accessors(data.shape())
            && self.json_lacks_to_json(value, plain.to_json))
        .then(|| self.shape_enumerable_slots(value))
        .flatten()
        .and_then(|slots| {
            slots
                .iter()
                .map(|&(atom, slot)| {
                    self.plain_named_key(atom).then(|| {
                        let start = plain.key_text.len() as u32;
                        self.write_json_key(atom, &mut plain.key_text);
                        plain.key_text.push(b':');
                        JsonMember { slot, key: (start, plain.key_text.len() as u32) }
                    })
                })
                .collect::<Option<std::rc::Rc<[JsonMember]>>>()
        });
        plain.object_plans.insert(key, plan.clone());
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
        // The plain writer keeps its (shape, prototype) plans between calls; a prototype that gains or
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
