//! Dead field stores in a lane region. A store to a field no program code reads (`AotFacts::field_has_reader`) emits
//! nothing: the closed world proves no read, enumeration or setter observes the value, so the store has no effect, and a
//! pure value computation whose only use was the store is removed by Cranelift as unused.

use super::*;

impl Lowering<'_, '_> {
    /// Whether the store at `pc` of field `atom` into the object in register `base` is dead. The solve names every class
    /// the object may be (`solved_classes`), and no program code reads the field of any of them. The base must be a
    /// proven object, so a store into a nullish base, which throws, is kept. A name an accessor defines is read by
    /// `Readers::accessor_names`, so a dead store never runs a setter.
    pub(crate) fn field_store_unread(&self, pc: usize, base: Register, atom: u32) -> bool {
        let Some(facts) = self.vm.facts else { return false };
        let Some(classes) = self.solved_classes(pc) else { return false };
        self.solved_cell(pc, base, false).ty.is_direct_object_family()
            && classes.ids().iter().all(|&class| {
                !facts.field_has_reader(quench_runtime::typed_ir::FactKey::Field { class, instance: true, atom })
            })
    }
}
