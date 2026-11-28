//! A guarded receiver's layout projection, reused while no admitted effect can change it.

use super::*;
use quench_runtime::staged::Slots;
use quench_runtime::staged::Layout;

/// A projection for a guarded receiver and exact shape: `staged::own_layout`'s residual. It may
/// only be reused while no admitted effect can change its layout, arena base or object lifetime.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct CheckedObject(Layout<ir::Value, ir::Value>, ShapeFact);

impl CheckedObject {
    /// The proof of a layout a staged read established, under the shape fact its guard made.
    pub(crate) fn of(layout: Layout<ir::Value, ir::Value>, fact: ShapeFact) -> Self { Self(layout, fact) }

    /// The shape fact the layout's guard made.
    pub(crate) fn fact(self) -> ShapeFact { self.1 }

    /// A flat store can reuse the range proof; packed writes use the VM's
    /// narrow-value encoder and layout-preserving barrier helper instead.
    pub(crate) fn arena_address(self, builder: &mut FunctionBuilder<'_>, slot: u16, miss: ir::Block) -> Option<ir::Value> {
        debug_assert!(usize::from(slot) < self.0.storage_len);
        let base = match self.0.slots {
            Slots::Packed(_) | Slots::Narrow(_) => return None,
            Slots::Flat(base) => base,
            Slots::Either { inline, flat, .. } => {
                let is_flat = emit(builder).raw_compare_imm(Relation::Equal, inline, 0);
                emit_js_guard(builder, is_flat, miss);
                flat
            }
        };
        Some(emit(builder).raw_binary_imm(Binary::Add, base, i64::from(slot) * quench_runtime::tier_surface::NATIVE_OBJECT_LAYOUT.property_value_size as i64))
    }

    /// The projection of the record `record` whose `storage_len` values the arena at `base` holds
    /// behind its extras (`staged::arena_extras_layout`), as a global object by name does: its
    /// shape is held by a dependency (`fact`, `HeldLayouts::hold`), so only the representation is tested.
    pub(crate) fn prove_arena_extras(builder: &mut FunctionBuilder<'_>, (record, fact): (ir::Value, ShapeFact), inline: ir::Value,
        base: ir::Value, storage_len: usize, miss: ir::Block) -> Option<Self> {
        let mut stage = Emit { builder, property_values: Some(base), gate: None, payload: None, facts: Default::default(), fresh: false };
        quench_runtime::staged::arena_extras_layout(&mut stage, record, inline, storage_len, miss)
            .map(|layout| Self(layout, fact))
    }

    /// The layout this proof carries, for a staged read of fresh contents through it.
    pub(crate) fn layout(self) -> Layout<ir::Value, ir::Value> { self.0 }
}
