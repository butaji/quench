//! The reads `GetField` records keep as their hints beside the site caches: a prototype's
//! method (`HolderHint`) and a getter's own-field return (`GetterHint`).
use super::*;

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
}

impl PrototypeHints {
    pub(super) fn clear(&mut self) {
        self.holder.clear();
        self.getter.clear();
    }

    /// Retires the watch of every hint whose prototype `live` no longer holds: a hint names its
    /// prototype by record id, which an allocation may give another object once it is swept.
    pub(super) fn retire_dead(&mut self, live: impl Fn(Value) -> bool) {
        let holders = self.holder.iter().map(|hint| (hint.holder, &hint.watch));
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
