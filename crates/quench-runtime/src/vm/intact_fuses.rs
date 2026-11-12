//! The interpreter's reads of the intrinsic-builtin fuses (`code_dependencies::IntactFuse`):
//! a use reads one fuse bit; the first use for a realm's holder, and the first after a layout
//! change of it, proves the fuse by one lookup.

use super::property_key::PropertyKey;
use super::*;
use crate::code_dependencies::IntactFuse;

/// How a fuse's holder names its method.
#[derive(Clone, Copy)]
enum FuseKey {
    IteratorSymbol,
    /// `next`, of an iterator prototype.
    Next,
    Named(&'static str),
}

/// Each fuse's key and intrinsic, in `IntactFuse` order.
const FUSES: [(FuseKey, Native); IntactFuse::COUNT] = [
    (FuseKey::IteratorSymbol, Native::ArrayValues),
    (FuseKey::Next, Native::ArrayIteratorNext),
    (FuseKey::IteratorSymbol, Native::SetValues),
    (FuseKey::Next, Native::IteratorNext),
    (FuseKey::IteratorSymbol, Native::MapEntries),
    (FuseKey::Next, Native::IteratorNext),
    (FuseKey::IteratorSymbol, Native::StringValues),
    (FuseKey::Next, Native::StringIteratorNext),
    (FuseKey::Named("charCodeAt"), Native::StringCharCodeAt),
    (FuseKey::Named("includes"), Native::StringIncludes),
    (FuseKey::Named("test"), Native::RegExpTest),
    (FuseKey::Named("exec"), Native::RegExpExec),
];

impl<H: Host> Vm<H> {
    /// Whether `fuse` holds in the current realm: its holder keeps the key as an own data
    /// property holding the realm's intrinsic.
    #[inline(always)]
    pub(super) fn fuse_intact(&mut self, fuse: IntactFuse) -> bool {
        let holder = self.fuse_holder(fuse);
        match self.heap.dependencies().fuses().intact(fuse, holder) {
            Some(intact) => intact,
            None => self.prove_fuse(fuse, holder),
        }
    }

    /// The current realm's object that holds `fuse`'s method.
    #[inline(always)]
    fn fuse_holder(&self, fuse: IntactFuse) -> Value {
        match fuse {
            IntactFuse::ArrayIterator => self.array_proto,
            IntactFuse::ArrayIteratorNext => self.array_iterator_proto,
            IntactFuse::SetIterator => self.set_proto,
            IntactFuse::SetIteratorNext => self.set_iterator_proto,
            IntactFuse::MapIterator => self.map_proto,
            IntactFuse::MapIteratorNext => self.map_iterator_proto,
            IntactFuse::StringIterator | IntactFuse::StringCharCodeAt | IntactFuse::StringIncludes => self.string_proto,
            IntactFuse::StringIteratorNext => self.string_iterator_proto,
            IntactFuse::RegExpTest | IntactFuse::RegExpExec => self.regexp_proto,
        }
    }

    pub(super) fn iterator_symbol_key(&self) -> Option<PropertyKey> {
        self.well_known_symbols.get(WellKnownSymbol::Iterator).map(PropertyKey::symbol)
    }

    /// `fuse`'s key and the intrinsic it holds.
    fn fuse_property(&self, fuse: IntactFuse) -> Option<(PropertyKey, Native)> {
        let (key, native) = FUSES[fuse as usize];
        let key = match key {
            FuseKey::IteratorSymbol => self.iterator_symbol_key()?,
            FuseKey::Next => PropertyKey::string(self.runtime_atoms.next),
            FuseKey::Named(name) => PropertyKey::string(self.lookup_atom(name)?),
        };
        Some((key, native))
    }

    /// Proves `fuse` for `holder` by one lookup, after making the holder a watched prototype
    /// whose writes and layout changes the fuse hears. A holder that cannot be watched answers
    /// without a proof.
    #[cold]
    #[inline(never)]
    fn prove_fuse(&mut self, fuse: IntactFuse, holder: Value) -> bool {
        self.heap.mark_prototype(holder);
        let slot = self.fuse_slot(fuse, holder);
        if let Some(subject) = self.heap.object(holder).and_then(|data| data.watched_subject()) {
            let slot = slot.and_then(|slot| u32::try_from(slot).ok());
            self.heap.dependencies_mut().fuses_mut().prove(fuse, holder, subject, slot);
        }
        slot.is_some()
    }

    /// The slot where `holder` keeps `fuse`'s intrinsic as an own data property, if it does.
    fn fuse_slot(&self, fuse: IntactFuse, holder: Value) -> Option<usize> {
        let (key, native) = self.fuse_property(fuse)?;
        let data = self.object_data(holder)?;
        let slot = self.property_shape_slot(data.shape(), key)?;
        if self.slot_attributes(data, slot)?.accessor {
            return None;
        }
        let value = self.heap.property_get(data, slot)?;
        self.is_current_realm_native(value, native).then_some(slot)
    }

    /// Whether GetIterator on `source` calls the realm's intrinsic `Array.prototype.values`
    /// and each step of the iterator it makes calls the intrinsic
    /// `%ArrayIteratorPrototype%.next`, every Get on the way reading a data property: an
    /// ordinary Array with no own `@@iterator` whose prototype is `Array.prototype`, while
    /// both fuses hold (`IntactFuse::ArrayIterator`, `IntactFuse::ArrayIteratorNext`).
    pub(super) fn array_iteration_intact(&mut self, source: Value) -> bool {
        let Some(array) = self.heap.array(source) else {
            return false;
        };
        if array.is_arguments_object() || array.has_side_descriptors() || array.proto() != self.array_proto {
            return false;
        }
        let shape = array.shape();
        (self.array_iteration_shape == Some(shape) || self.array_shape_lacks_iterator(shape))
            && self.fuse_intact(IntactFuse::ArrayIterator)
            && self.fuse_intact(IntactFuse::ArrayIteratorNext)
    }

    /// Whether array shape `shape` has no own `@@iterator`, remembered for a shape that keeps
    /// its properties (no dictionary) until shape compaction renumbers the shapes.
    #[cold]
    #[inline(never)]
    fn array_shape_lacks_iterator(&mut self, shape: u32) -> bool {
        let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::Iterator) else {
            return false;
        };
        let lacks = self.property_shape_slot(shape, PropertyKey::symbol(symbol)).is_none();
        if lacks && !self.shape_is_dictionary(shape) {
            self.array_iteration_shape = Some(shape);
        }
        lacks
    }

    /// Whether each of `builtins` is this realm's own method, an own data property of the
    /// realm's prototype that holds it (its fuse): a host step computing what guest code
    /// calling them would compute then calls nothing a script could observe.
    pub(crate) fn embedding_builtins_intact(&mut self, builtins: &[crate::IntactBuiltin]) -> bool {
        builtins.iter().all(|builtin| {
            self.fuse_intact(match builtin {
                crate::IntactBuiltin::StringCharCodeAt => IntactFuse::StringCharCodeAt,
                crate::IntactBuiltin::StringIncludes => IntactFuse::StringIncludes,
                crate::IntactBuiltin::RegExpTest => IntactFuse::RegExpTest,
                crate::IntactBuiltin::RegExpExec => IntactFuse::RegExpExec,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::test_host::TestHost;
    use crate::{Engine, Runtime};

    fn printed(source: &str) -> Vec<String> {
        crate::test_host::on_worker_stack(|| {
            let host = TestHost::default();
            let program = Engine::specialize(source, "fuses.js").unwrap();
            Runtime::new(host.clone()).execute(&program).unwrap();
            host.lines().clone()
        })
    }

    #[test]
    fn array_iteration_fuses_pop_on_their_slot_writes_and_survive_unrelated_layout_changes() {
        let source = r#"
const out = [];
const spread = (xs) => [...xs].join("");
const forOf = (xs) => { let s = ""; for (const x of xs) s += x; return s; };
for (let i = 0; i < 200; i++) { spread([1, 2]); forOf([1, 2]); }
Array.prototype.extra = function () {}; Object.defineProperty(Array.prototype, "other", { value: 1, configurable: true });
out.push(spread([1, 2]), forOf([3, 4]));
const proto = Object.getPrototypeOf([][Symbol.iterator]()); const next = proto.next;
let steps = 0; proto.next = function () { steps++; return next.call(this); };
out.push(spread([5, 6]), forOf([7]), steps); proto.next = next;
const values = Array.prototype[Symbol.iterator];
Array.prototype[Symbol.iterator] = function* () { yield "own"; };
const [first] = [8, 9]; out.push(spread([1, 2]), forOf([3]), first, Math.max(...[1, 2]));
Array.prototype[Symbol.iterator] = values; out.push(spread([1, 2]), forOf([3, 4]));
delete Array.prototype[Symbol.iterator];
try { spread([1]); out.push("no error"); } catch (e) { out.push(e.constructor.name); }
Array.prototype[Symbol.iterator] = values; out.push(spread([4, 5]));
print(out.join(" "));
"#;
        assert_eq!(printed(source), ["12 34 56 7 5 own own own NaN 12 34 TypeError 45"]);
    }

    #[test]
    fn dense_view_searches_and_join_read_what_the_generic_algorithms_read() {
        let source = r#"
const out = [];
const a = [1, "a", NaN, undefined, null, -0, 10n, , "b"];
for (const s of [1, "a", NaN, undefined, 0, 10n, "b", "z"]) out.push([a.includes(s), a.indexOf(s), a.lastIndexOf(s), a.includes(s, 6)].join());
Array.prototype[7] = "p";
out.push(a.includes("p"), a.indexOf("p"), a.lastIndexOf("p"));
delete Array.prototype[7];
const shrink = [1, 2, 3, 4];
out.push(shrink.indexOf(4, { valueOf() { shrink.length = 2; return 0; } }));
const getter = [1, 2, 3]; Object.defineProperty(getter, 1, { get() { getter.length = 1; return 2; } });
out.push(getter.includes(3), getter.indexOf(3));
out.push([1, "a", null, undefined, true, -0, 1.5, , "é"].join("-"));
const log = []; const o = { toString() { log.push("o"); return "O"; } };
out.push([1, o, 3].join(","), log.join());
print(out.join(" | "));
"#;
        assert_eq!(printed(source), [r#"true,0,0,false | true,1,1,false | true,-1,-1,false | true,3,3,true | true,5,5,false | true,6,6,true | true,8,8,true | false,-1,-1,false | true | 7 | 7 | -1 | false | -1 | 1-a---true-0-1.5--é | 1,O,3 | o"#]);
    }

    #[test]
    fn intact_collection_sources_copy_and_iterate_as_the_protocol_does() {
        let source = r#"
const out = [];
const m = new Map([[1, "a"], [NaN, "n"], [-0, "z"]]);
const copy = new Map(m); copy.set("new", 1);
out.push([...copy].join(";"), m.size, copy.size, Object.is([...copy.keys()][2], -0));
const holes = new Set(); for (let i = 0; i < 30; i++) holes.add(i); for (let i = 0; i < 30; i += 3) holes.delete(i);
const hc = new Set(holes); hc.add(0); out.push([...hc].join(), hc.size, holes.size);
out.push([...new Set("hello")].join(""), Array.from("a\u{1F600}b").length, [...new Map([[1, 2]])].join());
const calls = []; const set = Map.prototype.set;
Map.prototype.set = function (k, v) { calls.push(k); return set.call(this, k, v); };
out.push(new Map(new Map([[1, 2], [3, 4]])).size, calls.join()); Map.prototype.set = set;
const next = Object.getPrototypeOf(new Set()[Symbol.iterator]()).next; let steps = 0;
Object.getPrototypeOf(new Set()[Symbol.iterator]()).next = function () { steps++; return next.call(this); };
out.push(new Set(new Set([1, 2])).size, [...new Set([1])].length, steps);
Object.getPrototypeOf(new Set()[Symbol.iterator]()).next = next;
const iterator = String.prototype[Symbol.iterator];
String.prototype[Symbol.iterator] = function* () { yield "X"; };
out.push([...new Set("ab")].join(""), Array.from("ab").join(""));
String.prototype[Symbol.iterator] = iterator;
const own = new Set([1, 2]); own[Symbol.iterator] = function* () { yield "own"; };
out.push([...own].join(), [...new Set(own)].join());
const seen = []; const growing = new Set([1]); for (const v of growing) { seen.push(v); if (v < 4) growing.add(v + 1); }
out.push(seen.join(""));
const mm = new Map([[1, 1], [2, 2], [3, 3]]); const e = []; for (const [k] of mm) { e.push(k); if (k === 1) mm.delete(2); }
out.push(e.join(""));
print(out.join(" | "));
"#;
        assert_eq!(printed(source), [r#"1,a;NaN,n;0,z;new,1 | 3 | 4 | false | 1,2,4,5,7,8,10,11,13,14,16,17,19,20,22,23,25,26,28,29,0 | 21 | 20 | helo | 3 | 1,2 | 2 | 1,3,1,3 | 2 | 1 | 5 | X | X | own | own | 1234 | 13"#]);
    }
}
