//! The reads `GetField` records keep as their hints beside the site caches: a prototype's method (`HolderHint`).
//! A getter's own-field and an own constant's hints named an object record; every ordinary object is a class
//! cell now, so they are gone (S7g).
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

/// The hints of every `GetField` record, by index within their kind.
#[derive(Default)]
pub(super) struct PrototypeHints {
    pub(super) holder: Vec<HolderHint>,
}

impl PrototypeHints {
    pub(super) fn clear(&mut self) {
        self.holder.clear();
    }

    /// Retires the watch of every hint whose prototype `live` no longer holds: a hint names its
    /// prototype by record id, which an allocation may give another object once it is swept.
    pub(super) fn retire_dead(&mut self, live: impl Fn(Value) -> bool) {
        self.holder
            .iter()
            .map(|hint| (hint.holder, &hint.watch))
            .filter(|(holder, _)| !live(*holder))
            .for_each(|(_, watch)| watch.retire());
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
