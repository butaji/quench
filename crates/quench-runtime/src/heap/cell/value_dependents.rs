//! The bit that names the objects a folded construct value depends on.

use super::*;

impl Object {
    /// Whether a folded property value names this object (`Heap::value_epoch`).
    pub(crate) fn has_value_dependents(&self) -> bool {
        self.element_facts().has(ElementFacts::VALUE_DEPENDENTS)
    }
    pub(crate) fn mark_value_dependents(&mut self) {
        self.mark_element_fact(ElementFacts::VALUE_DEPENDENTS);
    }
}
