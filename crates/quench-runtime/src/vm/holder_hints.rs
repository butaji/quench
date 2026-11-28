//! The reads `GetField` records keep as their hints beside the site caches: a prototype's
//! method (`HolderHint`) and a getter's own-field return (`GetterHint`).
use super::*;
use crate::staged::holder::concrete_own_constant_read;

/// A `GetField` record's read of `atom` that a receiver's immediate prototype answered
/// (`Vm::holder_hint_hit`, `Vm::string_hint_hit`): a receiver whose whole auxiliary word, exact
/// shape and storage form, is `receiver` (`Object::hint_shape`), which lacks the name
/// (`NO_FIELD_RECEIVER` for a string primitive, whose own names a site that reads
/// `String.prototype` does not read), whose prototype is `holder`, read `value` under
/// `watch`, which a change to the holder's slot values or its own layout retires
/// (`heap::watch_prototype_values`): the holder is the receiver's immediate prototype, so no
/// other prototype stands between them.
#[derive(Clone)]
pub(super) struct HolderHint {
    /// The receiver's header words (`Object::guard_word`): its shape and storage form with
    /// its prototype link, `holder`'s.
    pub(super) guard: u64,
    pub(super) atom: Atom,
    pub(super) holder: Value,
    pub(super) value: Value,
    pub(super) watch: crate::heap::PrototypeValueWatch,
}

/// How many times a record's `GetterHint` is made again before its site is left to the general
/// read: a site that keeps meeting receivers of other shapes gains nothing from a hint.
const GETTER_HINT_REMAKES: u8 = 2;

/// A `GetField` record's read of `atom` whose getter, on the immediate prototype `holder`,
/// returns the own field at `slot` of a receiver whose header words are `guard` (`return
/// this.field`), under `watch` as for a `HolderHint`. `remakes` counts the times the record
/// made its hint again.
#[derive(Clone)]
pub(super) struct GetterHint {
    guard: u64,
    atom: Atom,
    pub(super) holder: Value,
    slot: u16,
    remakes: u8,
    watch: crate::heap::PrototypeValueWatch,
}

/// The hints of every `GetField` record, by index within their kind.
#[derive(Default)]
pub(super) struct PrototypeHints {
    pub(super) holder: Vec<HolderHint>,
    pub(super) getter: Vec<GetterHint>,
    /// Own constants (`Vm::own_constant_hint_hit`): a `HolderHint` whose holder is the receiver
    /// itself and whose guard is its record's address.
    pub(super) own: Vec<HolderHint>,
}

impl PrototypeHints {
    pub(super) fn clear(&mut self) {
        self.holder.clear();
        self.getter.clear();
        self.own.clear();
    }

    /// Retires the watch of every hint whose prototype `live` no longer holds: a hint names its
    /// prototype by record id, which an allocation may give another object once it is swept.
    pub(super) fn retire_dead(&mut self, live: impl Fn(Value) -> bool) {
        let holders = self.holder.iter().chain(&self.own).map(|hint| (hint.holder, &hint.watch));
        let getters = self.getter.iter().map(|hint| (hint.holder, &hint.watch));
        holders.chain(getters).filter(|(holder, _)| !live(*holder)).for_each(|(_, watch)| watch.retire());
    }
}

impl Vm {
    /// The operands of getter hint `index` for a read of `atom` (`IcOp::GetterOwn`): the guard
    /// word of the receivers it answers, its watch's address and the own slot it returns, for a
    /// receiver of the hint's shape and prototype while the watch holds. `None` for any other
    /// name, or no such hint.
    #[inline(always)]
    pub fn getter_hint_operands(&self, index: u16, atom: Atom) -> Option<(u64, u64, u16)> {
        let hint = self.holder_hints.getter.get(usize::from(index))?;
        (hint.atom == atom).then(|| (hint.guard, std::rc::Rc::as_ptr(&hint.watch) as u64, hint.slot))
    }

    /// Makes the own field `slot` of the direct object `object`, which its immediate prototype's
    /// getter returns for a read of `atom`, a record's hint: in place of `owned`, the hint the
    /// record holds, else as a new one, its index below `limit`. `None` once the record has made
    /// its hint again too often, or for another receiver. The prototype is marked first, so a
    /// change to its values retires the hint's watch.
    pub fn hold_getter_hint(&mut self, owned: Option<u16>, atom: Atom, object: Value, slot: u16, limit: u16) -> Option<u16> {
        let receiver = self.heap.object_record(object)?;
        let (guard, prototype) = (receiver.guard_word(), receiver.proto());
        let owned = owned.filter(|&index| usize::from(index) < self.holder_hints.getter.len());
        let remakes = owned.map_or(0, |index| self.holder_hints.getter[usize::from(index)].remakes + 1);
        if remakes > GETTER_HINT_REMAKES {
            return None;
        }
        self.heap.mark_prototype(prototype);
        let watch = crate::heap::watch_prototype_values(self.heap.object(prototype)?.exact_shape())?;
        let hint = GetterHint { guard, atom, holder: prototype, slot, remakes, watch };
        if let Some(index) = owned {
            self.holder_hints.getter[usize::from(index)] = hint;
            return Some(index);
        }
        let index = u16::try_from(self.holder_hints.getter.len()).ok().filter(|&index| index < limit)?;
        self.holder_hints.getter.push(hint);
        Some(index)
    }
}

impl Vm {
    /// What the own-constant hint `index` answers for a read of `atom` from the direct object
    /// `object`: the value the hint recorded, for the hinted object itself while nothing has
    /// written its values or changed its layout. `None` for any other receiver or name.
    #[inline(always)]
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn own_constant_hint_hit(&self, index: u16, atom: Atom, object: Value) -> Option<Value> {
        let hint = self.holder_hints.own.get(usize::from(index))?;
        let receiver = self.heap.object_record(object)?;
        // SAFETY: the hint's watch is live as long as the hint.
        (hint.atom == atom).then(|| unsafe {
            concrete_own_constant_read(receiver, hint.guard, std::rc::Rc::as_ptr(&hint.watch), hint.value)
        })?
    }

    /// Makes `value`, the own field `atom` of the direct object `object` that a site keeps
    /// reading from that one object, a record's own-constant hint: in place of `owned`, else as
    /// a new one below `limit`. The object is marked (`Heap::mark_own_constant`), so a write to
    /// any of its values or a change of its layout retires the watch and clears the mark; a
    /// site whose hint retired should not make one again (the object is written after all).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn hold_own_constant_hint(&mut self, owned: Option<u16>, atom: Atom, object: Value, value: Value, limit: u16) -> Option<u16> {
        self.heap.mark_own_constant(object)?;
        let record = self.heap.object_record(object)?;
        let (guard, shape) = (record as *const crate::heap::Object as u64, record.exact_shape());
        let watch = crate::heap::watch_prototype_values(shape)?;
        let hint = HolderHint { guard, atom, holder: object, value, watch };
        if let Some(index) = owned.filter(|&index| usize::from(index) < self.holder_hints.own.len()) {
            self.holder_hints.own[usize::from(index)] = hint;
            return Some(index);
        }
        let index = u16::try_from(self.holder_hints.own.len()).ok().filter(|&index| index < limit)?;
        self.holder_hints.own.push(hint);
        Some(index)
    }

    /// Whether own-constant hint `index` is held and its watch still published.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn own_constant_hint_live(&self, index: u16) -> bool {
        self.holder_hints.own.get(usize::from(index)).is_some_and(|hint| hint.watch.is_published())
    }

    /// The first live own-constant hint for a read of `atom` (`own_constant_hint_live`): the
    /// constant-marked object it holds, its exact shape, which a write to the object retires
    /// (`Object::note_constant_write`), and the value the member held. A lane body folds the read
    /// under an identity guard on the object, so a hint for another object only costs the guard.
    /// Every atom's answer from `own_constant_of` that holds now, by atom: what a lowering off
    /// the program's thread may fold (`FactTables`).
    pub fn own_constants(&self) -> std::collections::BTreeMap<Atom, crate::vm::engine_types::OwnConstant> {
        let mut constants = std::collections::BTreeMap::new();
        for hint in self.holder_hints.own.iter().filter(|hint| hint.watch.is_published()) {
            if !constants.contains_key(&hint.atom) && let Some(constant) = self.own_constant_of(hint.atom) {
                constants.insert(hint.atom, constant);
            }
        }
        constants
    }

    pub(super) fn own_constant_of(&self, atom: Atom) -> Option<crate::vm::engine_types::OwnConstant> {
        let hint = self.holder_hints.own.iter().find(|hint| hint.atom == atom && hint.watch.is_published())?;
        let shape = self.heap.object(hint.holder)?.exact_shape();
        Some(crate::vm::engine_types::OwnConstant { object: hint.holder, shape, value: hint.value })
    }
}

thread_local! {
    /// A watch no write retires (`never_retired`).
    static NEVER_RETIRED: crate::heap::PrototypeValueWatch = crate::code_dependencies::new_dependency();
}

/// The address of a watch no write retires, for a held read whose answer no prototype value
/// decides (an absent inherited name, which the entry's proof alone keeps). It lives as long as
/// the thread, which outlives every read given it.
pub(super) fn never_retired() -> *const crate::code_dependencies::PublishedCode<()> {
    NEVER_RETIRED.with(std::rc::Rc::as_ptr)
}
