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
