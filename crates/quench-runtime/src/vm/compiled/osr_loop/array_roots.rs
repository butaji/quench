//! A lifetime region's indexed receivers: each access reads through the entry array that
//! roots it. A temporary slot may hold a copy of an entry array, and must never be read as
//! the unrelated value that slot held at entry.

use super::slots::{self, Slot};
use super::*;
use std::collections::BTreeSet;

/// The entry array a receiver's definition roots at: an entry register no instruction
/// defines, or a `Move` copy of one (`instruction.register_b()`), recursively.
fn array_root(
    flow: &LoopRegion,
    values: &Webs,
    pc: u32,
    register: Register,
    visiting: &mut BTreeSet<(u32, Register)>,
) -> Option<Register> {
    if !visiting.insert((pc, register)) {
        return None;
    }
    let definitions = values.reaching[(pc - flow.header) as usize].get(&register)?;
    let mut root = None;
    for definition in definitions {
        let candidate = if let Some(entry) = flow.entry_register(*definition) {
            if flow.instructions.iter().any(|instruction| instruction.defined_register() == Some(entry)) {
                return None;
            }
            entry
        } else {
            let slot: Slot = slots::of(flow.entry_count(), *definition)?;
            if slot.second {
                return None;
            }
            let instruction = *flow.instructions.get(slot.offset)?;
            if !instruction.op().is_move() || instruction.result_flags() != 0 {
                return None;
            }
            array_root(flow, values, flow.header + slot.offset as u32, instruction.register_b(), visiting)?
        };
        if root.is_some_and(|root| root != candidate) {
            return None;
        }
        root = Some(candidate);
    }
    visiting.remove(&(pc, register));
    root
}

/// `flow`'s instructions with each indexed access's receiver renamed to the entry array that
/// roots it. An access whose receiver reads a `GetIndex` result stays as it is (the region
/// pins that array where it reads it); any other unrooted receiver refuses the region.
pub(super) fn canonical_receivers(flow: &LoopRegion, values: &Webs) -> Option<Vec<WideInstruction>> {
    let mut canonical = flow.instructions.clone();
    for (offset, instruction) in canonical.iter_mut().enumerate() {
        // Field receivers rename as values do; only array accesses read through an entry
        // array's root.
        if !instruction.op().is_indexed_access() {
            continue;
        }
        if let Some(receiver) = object_reads(*instruction) {
            let root = match array_root(flow, values, flow.header + offset as u32, receiver, &mut BTreeSet::new()) {
                Some(root) => root,
                None if instruction.op().is_get_index()
                    && values.reaching[offset].get(&receiver).is_some_and(|definitions| {
                        !definitions.is_empty() && definitions.iter().all(|definition| {
                            slots::of(flow.entry_count(), *definition)
                                .filter(|slot| !slot.second)
                                .and_then(|slot| flow.instructions.get(slot.offset))
                                .is_some_and(|definition| definition.op().is_get_index())
                        })
                    }) => continue,
                None => return None,
            };
            let base = if instruction.op().is_get_index() { Operand::register(root).0 } else { root };
            *instruction = WideInstruction::new(instruction.op(), instruction.a(), base, instruction.c(), instruction.imm());
        }
    }
    Some(canonical)
}
