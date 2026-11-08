//! The interpreter's reads of the intrinsic-builtin fuses (`code_dependencies::IntactFuse`):
//! a use reads one fuse bit; the first use for a realm's holder, and the first after a layout
//! change of it, proves the fuse by one lookup.

use super::property_key::PropertyKey;
use super::*;
use crate::code_dependencies::IntactFuse;

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
            IntactFuse::StringCharCodeAt | IntactFuse::StringIncludes => self.string_proto,
            IntactFuse::RegExpTest | IntactFuse::RegExpExec => self.regexp_proto,
        }
    }

    /// `fuse`'s key and the intrinsic it holds.
    fn fuse_property(&self, fuse: IntactFuse) -> Option<(PropertyKey, Native)> {
        let named = |name: &str, native| self.lookup_atom(name).map(|atom| (PropertyKey::string(atom), native));
        match fuse {
            IntactFuse::ArrayIterator => self
                .well_known_symbols
                .get(WellKnownSymbol::Iterator)
                .map(|symbol| (PropertyKey::symbol(symbol), Native::ArrayValues)),
            IntactFuse::ArrayIteratorNext => Some((PropertyKey::string(self.runtime_atoms.next), Native::ArrayIteratorNext)),
            IntactFuse::StringCharCodeAt => named("charCodeAt", Native::StringCharCodeAt),
            IntactFuse::StringIncludes => named("includes", Native::StringIncludes),
            IntactFuse::RegExpTest => named("test", Native::RegExpTest),
            IntactFuse::RegExpExec => named("exec", Native::RegExpExec),
        }
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
}
