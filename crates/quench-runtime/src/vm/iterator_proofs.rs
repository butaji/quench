//! Proofs that a built-in iterator's prototype chain lacks a key (`iterator_absence_proof`),
//! so a destructuring close ends without the general lookup, and the intrinsic array iterator
//! a destructuring takes without `@@iterator`.

use super::property_key::PropertyKey;
use super::*;

impl Vm {
    /// Whether `iterator_absence_proof` proves that `proto`'s chain lacks `key`.
    #[inline(always)]
    pub(super) fn iterator_absence_proved(&self, proto: Value, key: PropertyKey) -> bool {
        let Some(atom) = key.key32() else {
            return false;
        };
        self.iterator_absence_proof.get().is_some_and(|(held, held_atom, cell)| held == proto && held_atom == atom && cell.holds())
    }

    /// `iterator_lacks_property`, proving for the next ask, when the iterator's prototype
    /// chain lacks `key`, that it does: each link becomes a prototype first, and the proof keeps
    /// the chain's cell, so a later change to one retires it (`Vm::chain_cell`).
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
        self.prove_iterator_absence(proto, atom);
        true
    }

    /// Holds that the chain from `proto` lacks the key `atom`, which the caller found, under the
    /// whole chain's cell: marking a link that was no prototype yet changes no layout, so the
    /// finding holds under the cell made after it.
    fn prove_iterator_absence(&mut self, proto: Value, atom: u32) {
        if let Some(cell) = self.chain_cell_to_end(proto) {
            self.iterator_absence_proof.set(Some((proto, atom, cell)));
        }
    }

    /// `iterator_close` of an iterator whose close the shapes end before any call
    /// (`iterator_lacks_property`), proving that for the next close; `false` for any other,
    /// whose close takes the general step.
    pub fn iterator_close_absent(&mut self, iterator: Value) -> bool {
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

    /// Whether an array pattern of `count` elements (`[a, b] = source`) reads `source`'s
    /// elements `0..count` with no observable step: the iteration is intact
    /// (`intact_spread_length`: a plain array of present data elements on the realm's
    /// `Array.prototype`, whose `@@iterator` and `next` are the intrinsics), it holds at least
    /// `count` elements, so each of the `count` steps yields its element and none reports
    /// done, and the array iterator's chain lacks `return`, so the close that follows calls
    /// nothing. `false` leaves the pattern to the iterator protocol.
    pub fn array_pattern_fits(&mut self, source: Value, count: usize) -> bool {
        if !self.intact_spread_length(source).is_some_and(|length| length >= count) {
            return false;
        }
        let key = PropertyKey::string(self.runtime_atoms.iterator_return);
        let proto = self.array_iterator_proto;
        if self.iterator_absence_proved(proto, key) {
            return true;
        }
        self.prove_array_iterator_lacks(key)
    }

    /// `array_pattern_fits`'s proof that the realm's array iterator prototype chain lacks
    /// `key`, kept for the next ask as `iterator_lacks_property_proving` keeps it.
    #[cold]
    #[inline(never)]
    fn prove_array_iterator_lacks(&mut self, key: PropertyKey) -> bool {
        let proto = self.array_iterator_proto;
        let Some(atom) = key.key32() else {
            return false;
        };
        if self.plain_property_lookup(proto, key) != Some(None) {
            return false;
        }
        self.prove_iterator_absence(proto, atom);
        true
    }

    /// Resolves `promise` with a fresh iterator result `{ value, done }`, as
    /// `promise_resolve_value` resolves it, without the read of the result's `then` where
    /// `then_absence_proved` shows its prototype chain lacking one.
    ///
    /// The skipped read observes nothing and finds nothing. The result is fresh, so no code
    /// has seen it: its own properties are exactly the data properties `value` and `done` of
    /// its literal shape, with no side descriptors, and its prototype is the realm's
    /// `%Object.prototype%`, the one `iterator_result` derives its header for. The proof
    /// found `then` in no link of that prototype's chain, neither in a shape nor in a side
    /// descriptor, under the cell of that chain; adding one to a link, as data or accessor,
    /// or changing a link's prototype, retires the cell first, so the proof
    /// lapses before a read could find anything. An absent `then` reads as `undefined`,
    /// which is not callable, so `resolve_promise_with_then` would fulfill the promise with
    /// the result, which is what happens here; a fresh result is never the promise itself.
    pub(super) fn resolve_iterator_result(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        value: Value,
        done: bool,
    ) -> Result<(), JsError> {
        let result = self.iterator_result(value, done)?;
        let prototype = self.realm_object_prototype(self.realm.globals);
        if self.then_absence_proved(prototype) {
            return self.promise_settle(p, promise, crate::heap::PromiseState::Fulfilled, result);
        }
        self.promise_resolve_value(p, promise, result)
    }

    /// Whether a fresh iterator result of the current realm lacks `then` along its prototype
    /// chain (`then_absence_proved`), so `resolve_iterator_result` fulfills its promise without
    /// the read, running no guest code.
    pub(super) fn iterator_results_then_free(&mut self) -> bool {
        let prototype = self.realm_object_prototype(self.realm.globals);
        self.then_absence_proved(prototype)
    }

    /// Whether `prototype`'s chain lacks `then` (`plain_property_lookup`), proving it for the
    /// next ask (`then_absence_proof`) as `iterator_lacks_property_proving` proves its keys:
    /// the proof holds the cell of the chain to its end (`chain_cell_to_end`), which a later
    /// layout change of any link retires.
    fn then_absence_proved(&mut self, prototype: Value) -> bool {
        if let Some((proved, cell)) = self.then_absence_proof
            && proved == prototype
            && cell.holds()
        {
            return true;
        }
        let key = PropertyKey::string(self.runtime_atoms.then);
        if self.plain_property_lookup(prototype, key) != Some(None) {
            return false;
        }
        // Marking a link changes no layout, so a cell made after the lookup holds its answer.
        self.then_absence_proof = self.chain_cell_to_end(prototype).map(|cell| (prototype, cell));
        true
    }

    /// A fresh array iterator of `kind` over the object `source`, as `%Array.prototype.values%`
    /// and its kin make one.
    pub fn array_iterator_of(&mut self, kind: IteratorKind, source: Value) -> Value {
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
