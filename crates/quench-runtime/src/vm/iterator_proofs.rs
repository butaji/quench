//! Proofs that a built-in iterator's prototype chain lacks a key (`iterator_absence_proof`),
//! so a destructuring close ends without the general lookup, and the intrinsic array iterator
//! a destructuring takes without `@@iterator`.

use super::property_key::PropertyKey;
use super::*;

impl<H: Host> Vm<H> {
    /// Whether `iterator_absence_proof` proves that `proto`'s chain lacks `key`.
    #[inline(always)]
    pub(super) fn iterator_absence_proved(&self, proto: Value, key: PropertyKey) -> bool {
        let Some(atom) = key.key32() else {
            return false;
        };
        self.iterator_absence_proof.get() == Some((proto, atom, crate::heap::prototype_layout_epoch()))
    }

    /// `iterator_lacks_property`, proving for the next ask, when the iterator's prototype
    /// chain lacks `key`, that it does: each link becomes a prototype first, so a later change
    /// to one advances the epoch and retires the proof.
    pub(super) fn iterator_lacks_property_proving(&mut self, iterator: Value, key: PropertyKey) -> bool {
        if !self.iterator_lacks_property(iterator, key) {
            return false;
        }
        let (Some(record), Some(atom)) = (self.heap.iterator(iterator), key.key32()) else {
            return true;
        };
        let proto = record.object.proto();
        if proto.is_null() || self.iterator_absence_proved(proto, key) {
            return true;
        }
        let epoch = crate::heap::prototype_layout_epoch();
        let mut link = proto;
        while !link.is_null() {
            self.heap.mark_prototype(link);
            link = self.heap.ordinary_object(link).map_or(Value::NULL, Object::proto);
        }
        if crate::heap::prototype_layout_epoch() == epoch {
            self.iterator_absence_proof.set(Some((proto, atom, epoch)));
        }
        true
    }

    /// `iterator_close` of an iterator whose close the shapes end before any call
    /// (`iterator_lacks_property`), proving that for the next close; `false` for any other,
    /// whose close takes the general step.
    pub(super) fn iterator_close_absent(&mut self, iterator: Value) -> bool {
        let target = self.iterator_close_target(iterator);
        let Some(IteratorRecord { object, .. }) = self.heap.iterator(target) else {
            return false;
        };
        let key = PropertyKey::string(self.runtime_atoms.iterator_return);
        // An iterator with no own property (the empty shape, no side table) above a proved
        // prototype: the proof alone answers, without the lookup of `key` in its shape.
        if object.shape() == 0 && !object.has_side_descriptors() && self.iterator_absence_proved(object.proto(), key) {
            return true;
        }
        self.iterator_lacks_property_proving(target, key)
    }

    /// Whether destructuring `value` with an array pattern of `elements` elements and no rest
    /// binds element `i` to `value[i]` and ends with no call: an ordinary Array whose iteration
    /// is intact (`array_iteration_intact`) with no indexed descriptor or sparse storage, whose
    /// first `elements` elements are present data elements, so each iterator step reads its own
    /// element and none is the last; then the pattern's close (the iterator is never done) finds
    /// no `return` on `%ArrayIteratorPrototype%`'s chain. Nothing here, nor in reading those
    /// elements, runs guest code.
    pub(super) fn array_pattern_reads_elements(&mut self, value: Value, elements: usize) -> bool {
        if !self.array_iteration_intact(value) {
            return false;
        }
        let Some(array) = self.heap.array(value) else {
            return false;
        };
        if array.has_indexed_descriptors() || self.heap.sparse_length(value).is_some() {
            return false;
        }
        let stored = array.elements();
        if stored.len() < elements || (0..elements).any(|index| stored.get(index).is_none_or(|element| element.is_deleted())) {
            return false;
        }
        let key = PropertyKey::string(self.runtime_atoms.iterator_return);
        let proto = self.array_iterator_proto;
        self.iterator_absence_proved(proto, key) || self.prove_chain_lacks(proto, key)
    }

    /// Whether the chain from `proto` lacks `key` in every link's shape, recording the proof
    /// (`iterator_absence_proof`) after making each link a prototype, as
    /// `iterator_lacks_property_proving` does.
    #[cold]
    #[inline(never)]
    fn prove_chain_lacks(&mut self, proto: Value, key: PropertyKey) -> bool {
        let Some(atom) = key.key32() else {
            return false;
        };
        let mut link = proto;
        while !link.is_null() {
            let Some(data) = self.heap.ordinary_object(link) else {
                return false;
            };
            if data.has_side_descriptors() || self.shape_is_dictionary(data.shape()) || self.property_shape_slot(data.shape(), key).is_some() {
                return false;
            }
            link = data.proto();
        }
        let epoch = crate::heap::prototype_layout_epoch();
        let mut link = proto;
        while !link.is_null() {
            self.heap.mark_prototype(link);
            link = self.heap.ordinary_object(link).map_or(Value::NULL, Object::proto);
        }
        if crate::heap::prototype_layout_epoch() == epoch {
            self.iterator_absence_proof.set(Some((proto, atom, epoch)));
        }
        true
    }

    /// A fresh array iterator of `kind` over the object `source`, as `%Array.prototype.values%`
    /// and its kin make one.
    pub(super) fn array_iterator_of(&mut self, kind: IteratorKind, source: Value) -> Value {
        self.heap.alloc_iterator(IteratorRecord {
            object: Self::empty_object(self.array_iterator_proto),
            source,
            kind,
            index: 0,
            done: false,
            ext: crate::heap::IteratorExt::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::test_host::TestHost;
    use crate::vm::Vm;
    use crate::Engine;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn run(source: &str) -> String {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(output.clone()));
        vm.execute(&Engine::specialize(source, "array_patterns.js").unwrap()).unwrap();
        output.borrow().join("\n")
    }

    #[test]
    fn array_patterns_by_index_keep_iterator_semantics() {
        // Patterns of local names read intact arrays by index; every case where the iterator
        // form could observe more takes that form.
        let source = r#"
"use strict";
function f() {
  const out = [];
  const m = new Map([["a", 1], ["b", 2]]);
  for (const [k, v] of m) out.push(k + v);
  let a = 1, b = 2; [a, b] = [b, a]; out.push(a, b);
  const [x, , z] = [7, 8, 9]; out.push(x, z);
  const [p, q] = [1]; out.push(String(q));
  const [h1, h2] = [1, , 3]; out.push(String(h2));
  const it = [5, 6]; it[Symbol.iterator] = function* () { yield 40; yield 41; };
  const [i1, i2] = it; out.push(i1, i2);
  const [c1, c2] = "xy"; out.push(c1, c2);
  let closed = 0;
  const ArrIt = Object.getPrototypeOf([][Symbol.iterator]());
  ArrIt.return = function () { closed++; return {}; };
  const [r1] = [1, 2]; out.push(r1, closed);
  delete ArrIt.return;
  const [r2] = [1, 2]; out.push(r2, closed);
  Array.prototype[1] = "proto";
  const [g1, g2] = [1, , 3]; out.push(g2);
  delete Array.prototype[1];
  try { const k1 = 0; [k1] = [5]; } catch (e) { out.push(e.constructor.name); }
  return out;
}
print(JSON.stringify(f()));
"#;
        assert_eq!(run(source), r#"["a1","b2",2,1,7,9,"undefined","undefined",40,41,"x","y",1,1,1,1,"proto","TypeError"]"#);
    }

    #[test]
    fn concat_and_slice_follow_spreadable_and_species() {
        let source = r#"
const a = [1, 2, 3, 4, 5], out = [];
out.push(a.slice(1, 3), a.slice(-2), [1, , 3].slice(0, 3).length, a.concat([6, 7], 8, [[9]]));
const b = [1, 2]; b[Symbol.isConcatSpreadable] = false; out.push([0].concat(b));
Array.prototype[Symbol.isConcatSpreadable] = false; out.push([0].concat([1, 2]));
delete Array.prototype[Symbol.isConcatSpreadable]; out.push([0].concat([1, 2]));
Object.prototype[Symbol.isConcatSpreadable] = false; out.push([0].concat([1, 2]));
delete Object.prototype[Symbol.isConcatSpreadable];
class MyA extends Array {} const m = MyA.from([1, 2, 3]);
out.push(m.slice(1) instanceof MyA, m.concat([4]) instanceof MyA);
print(JSON.stringify(out));
"#;
        assert_eq!(run(source), r#"[[2,3],[4,5],3,[1,2,3,4,5,6,7,8,[9]],[0,[1,2]],[[0],[1,2]],[0,1,2],[[0],[1,2]],true,true]"#);
    }

    #[test]
    fn entry_loops_share_a_pair_only_a_pattern_reads() {
        // A for-of whose value goes only to an array pattern may reuse one entry pair; any
        // value that escapes, a manual next(), or a patched array iterator sees fresh pairs.
        let source = r#"
"use strict";
function f() {
  const out = [], kept = [];
  const m = new Map([["a", 1], ["b", 2], ["c", 3]]);
  for (const [k, v] of m) out.push(k + v);
  for (const e of m) kept.push(e);
  out.push(kept[0] !== kept[1], JSON.stringify(kept));
  for (const [k, v] of m) for (const [k2, v2] of m) if (k === "a") out.push(k2 + v);
  for (const [i, x] of ["p", "q"].entries()) out.push(i + x);
  const it = m.entries(); for (const [k] of it) { const n = it.next(); if (!n.done) kept.push(n.value); }
  out.push(JSON.stringify(kept.slice(3)));
  const fns = []; for (let [k, v] of m) fns.push(() => k + v); out.push(fns.map(g => g()).join());
  const values = Array.prototype[Symbol.iterator], seen = [];
  Array.prototype[Symbol.iterator] = function () { seen.push(this); return values.call(this); };
  for (const [k, v] of m) out.push(k);
  Array.prototype[Symbol.iterator] = values;
  out.push(seen.length, seen[0] !== seen[1], JSON.stringify(seen));
  return out;
}
print(JSON.stringify(f()));
"#;
        assert_eq!(run(source), r#"["a1","b2","c3",true,"[[\"a\",1],[\"b\",2],[\"c\",3]]","a1","b1","c1","0p","1q","[[\"b\",2]]","a1,b2,c3","a","b","c",3,true,"[[\"a\",1],[\"b\",2],[\"c\",3]]"]"#);
    }
}
