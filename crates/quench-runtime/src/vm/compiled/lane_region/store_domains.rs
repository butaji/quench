//! The slot types a lane unit's raw stores may write.

use super::*;

/// The writes the raw stores of `instructions` may make, once per physical slot (shape and
/// slot), independently of cache-row binding availability: each store site's one proved
/// field's (`field` of its cache site) declared type.
pub(in crate::vm) fn store_domains<'a>(instructions: &[WideInstruction], field: impl Fn(u16) -> Option<&'a [NativeFieldLoad]>)
    -> Vec<(u32, u16, Type)> {
    let mut domains: Vec<(u32, u16, Type)> = Vec::new();
    for instruction in instructions.iter().filter(|instruction| instruction.op().is_field_store()) {
        let Some([field]) = field(instruction.cache_site_index()) else { continue };
        if field.ty == Type::NEVER { continue; }
        match domains.iter_mut().find(|(shape, slot, _)| (*shape, *slot) == (field.shape, field.slot)) {
            Some((_, _, domain)) => *domain = domain.join(field.ty),
            None => domains.push((field.shape, field.slot, field.ty)),
        }
    }
    domains
}

impl Lowering<'_, '_> {
    /// Admits at entry the writes the unit's raw stores may make (`store_domains`), each
    /// failing through `miss`. Profiled raw stores preserve their declared field type. An
    /// online body's plan registered those possible writes before it was lowered
    /// (`Vm::admit_lane_stores`); other code registers them here, at each entry. A frozen
    /// image has no plan, so it admits them at each entry, as offline code does.
    pub(super) fn admit_store_domains(&mut self, instructions: &[WideInstruction], miss: &mut Option<ir::Block>) {
        if self.vm.online.is_some() && !self.vm.frozen {
            return;
        }
        for (shape, slot, domain) in store_domains(instructions, |site| self.fields.get(&site).map(Vec::as_slice)) {
            let exit = *miss.get_or_insert_with(|| self.builder.create_block());
            let shape = self.builder.ins().iconst(types::I64, i64::from(shape));
            let slot = self.builder.ins().iconst(types::I64, i64::from(slot));
            self.admit_slot_writes(shape, slot, domain, exit);
        }
    }
}
