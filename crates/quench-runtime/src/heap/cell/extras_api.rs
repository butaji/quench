//! The extras reads and writes that a live class cell's view answers too (`CellView`, `CellViewMut`, in
//! `heap::object_view`). Each is the body of the `Object` accessor of the same meaning over the extras
//! themselves, so a record and a cell whose extras are by id (`ExtrasSpace`) share one definition.

use super::*;

impl ObjectExtras {
    /// Whether a prototype mark is set on these extras (`Object::was_used_as_prototype`).
    #[inline(always)]
    pub(crate) fn prototype_marked(&self) -> bool {
        self.prototype_lookup != PrototypeLookupState::Ordinary
    }

    /// Whether the prototype's lookup is ready (`Object::prototype_lookup_ready`).
    #[inline(always)]
    pub(crate) fn prototype_ready(&self) -> bool {
        self.prototype_lookup == PrototypeLookupState::Ready
    }

    /// Sets the prototype mark to setup mode (`Object::mark_as_prototype`).
    #[inline(always)]
    pub(crate) fn set_prototype_setup(&mut self) {
        self.prototype_lookup = PrototypeLookupState::Setup;
    }

    /// Readies the prototype's lookup, if it is marked (`Object::prepare_prototype_lookup`).
    #[inline(always)]
    pub(crate) fn prepare_prototype(&mut self) {
        if self.prototype_marked() {
            self.prototype_lookup = PrototypeLookupState::Ready;
        }
    }

    /// This prototype's subject in its heap's dependency table (`Object::watched_subject`).
    #[inline(always)]
    pub(crate) fn watched_subject(&self) -> Option<u32> {
        self.watch.subject()
    }

    /// Gives this prototype its subject in its heap's dependency table (`Object::watch_as`).
    #[inline(always)]
    pub(crate) fn set_watch(&mut self, watch: dependencies::Watch) {
        self.watch = watch;
    }

    /// The layout change of a prototype whose class has exact shape `shape`, made before it takes effect
    /// (`Object::note_prototype_layout_change`): the readers of its shape retire, and its watch (or, with no subject,
    /// the global epoch) retires. A prototype is marked, so an unmarked object's change retires nothing.
    pub(crate) fn note_prototype_layout_change(&self, shape: u32) {
        if !self.prototype_marked() {
            return;
        }
        prototype_values::note_layout_change(shape);
        if self.watched_subject().is_some() {
            self.watch.retire();
        } else {
            advance_prototype_layout_epoch();
        }
    }

    /// The brand of these extras (`Object::brand`).
    #[inline(always)]
    pub(crate) fn brand(&self) -> ObjectBrand {
        self.rare().map_or(ObjectBrand::Ordinary, |rare| rare.brand)
    }

    /// The element facts recorded beside the shape (`Object::element_facts`).
    #[inline(always)]
    pub(crate) fn element_facts(&self) -> ElementFacts {
        self.element_facts
    }

    /// Records element fact `bit` (`Object::mark_element_fact`).
    #[inline(always)]
    pub(crate) fn mark_element_fact(&mut self, bit: u8) {
        self.element_facts.0 |= bit;
    }

    /// The boxed primitive these extras hold, if any (`Object::boxed_primitive`).
    #[inline(always)]
    pub(crate) fn boxed_primitive(&self) -> Option<Value> {
        self.rare()?.boxed_primitive
    }

    /// Whether these extras hold data that a cached define must not overwrite past: a boxed primitive, stack or
    /// error data (`ObjectRef::holds_define_extras`).
    pub(crate) fn holds_define_data(&self) -> bool {
        self.boxed_primitive().is_some() || self.rare().is_some_and(|rare| rare.stack_data.is_some()) || self.brand() == ObjectBrand::Error
    }
}
