//! Root binding slots written once (`ResidualProgram::root_slots_written_once`): a slot whose
//! only write in the whole program is one store of the root body that runs at most once and
//! before any guest code can read the slot past its temporal dead zone. Once a read finds the
//! slot initialized, the value it found is the value every later read finds, so a read site
//! may keep it (`lane_load_root_constant`), as C reads a constant address.
//!
//! Every write a root slot can take is an instruction of this program: the root body's local
//! stores (any op addressing a local slot that is not a load), its environment copies
//! (`CloneEnv`), and a nested function's `StoreCapture` whose depth reaches the root. A by-name
//! store reaches a root binding only through a dynamic scope (direct `eval`, `with`), which
//! the derivation excludes for every function (`Function::own_names_resolve_statically`). A
//! function whose record is not built, or whose body is a first-call stub, is judged by the
//! captures the compiler laid its scopes out for (`ClosureReads::captures`), loads included.
// Read through `Vm::root_slot_written_once`, whose caller is the u-jit's root constant fold
// (`FactTables::root_constants`); the interpreter's root-constant load is not part of it.
#![allow(dead_code)]

use super::{Effect, Function, ImmediateRole, Op, ResidualProgram, WideInstruction, may_repeat};
use crate::bitset::BitSet;

/// What the root body writes to one slot, in code order (`root_written_once_slots`).
#[derive(Clone, Copy, Default)]
struct RootSlotWrites {
    /// The pc of its one value store, or of its first when `stores` is more than one.
    store: Option<usize>,
    stores: u8,
    /// Whether an `InitializeTdz` of it runs before any guest code: reads before its store
    /// then find the hole, which no read site keeps.
    tdz_before_guest_code: bool,
    /// Whether an `InitializeTdz` of it may run again, or after its store.
    tdz_late: bool,
}

impl RootSlotWrites {
    /// Whether these writes make the slot written once, with guest code first able to run at
    /// `first_guest_code` and the body's backward transfers `backward`.
    fn once(self, first_guest_code: usize, backward: &[(usize, usize)]) -> bool {
        let Some(store) = self.store else { return false };
        self.stores == 1
            && !self.tdz_late
            && !may_repeat(backward, store)
            && (store < first_guest_code || self.tdz_before_guest_code)
    }
}

/// Whether `instruction` may run guest code (a call, a getter, a resumption): any op that can
/// throw but `MakeClosure`, which only allocates.
fn runs_guest_code(instruction: WideInstruction) -> bool {
    instruction.op() != Op::MakeClosure && instruction.op().effect().contains(Effect::THROWS)
}

/// Whether `op`, which addresses a local slot, only reads it.
fn reads_local_slot(op: Op) -> bool {
    matches!(op, Op::LoadLocal | Op::LoadLocalPlain | Op::LoadEnvLocal | Op::ArgumentsLength | Op::ArgumentGet)
}

/// `ResidualProgram::root_slots_written_once`'s answer: the slots, and the functions it judged
/// by their captures because their records were not built (`unbuilt`). A capture table counts
/// loads as stores, so once one of those records is built, a derivation again may find more
/// slots written once: never fewer, as a built record's stores are among its captures.
pub(crate) struct RootWrittenOnce {
    pub(crate) once: BitSet,
    pub(crate) unbuilt: Box<[u32]>,
}

impl ResidualProgram {
    /// The root binding slots this program writes once (module comment), among the root's
    /// named locals; empty where any function may reach a scope by name or is not described.
    /// Callers add the slot's rule (`RootSlotRule::Binding`) and that the root body runs once.
    pub(crate) fn root_slots_written_once(&self) -> RootWrittenOnce {
        let Some(root) = self.function_get(0) else { return RootWrittenOnce { once: BitSet::new(0), unbuilt: Box::default() } };
        let locals = root.local_atoms.len();
        let mut once = self.root_written_once_slots(root, locals);
        let mut unbuilt = Vec::new();
        if !self.nested_functions_keep(&mut once, &mut unbuilt) {
            return RootWrittenOnce { once: BitSet::new(locals), unbuilt: Box::default() };
        }
        RootWrittenOnce { once, unbuilt: unbuilt.into() }
    }

    /// Whether the record of any of `functions` is built now: a derivation that judged them by
    /// their captures (`RootWrittenOnce::unbuilt`) may find more slots written once.
    pub(crate) fn any_record_built(&self, functions: &[u32]) -> bool {
        functions.iter().any(|&function| self.functions.get(function as usize).is_some_and(|slot| slot.record().is_some()))
    }

    /// The root body's half of `root_slots_written_once`.
    fn root_written_once_slots(&self, root: &Function, locals: usize) -> BitSet {
        let mut once = BitSet::new(locals);
        if !root.own_names_resolve_statically(self) {
            return once;
        }
        let code: Vec<WideInstruction> = root.instructions().collect();
        let backward = root.backward_transfers();
        let first_guest_code = code.iter().position(|&instruction| runs_guest_code(instruction)).unwrap_or(code.len());
        let mut writes = vec![RootSlotWrites::default(); locals];
        for (pc, &instruction) in code.iter().enumerate() {
            note_root_write(root, &mut writes, pc, instruction, first_guest_code, &backward);
        }
        for (slot, slot_writes) in writes.iter().enumerate() {
            if slot_writes.once(first_guest_code, &backward) {
                once.insert(slot);
            }
        }
        once
    }

    /// Removes from `once` every slot a function other than the root may store; `false` where
    /// one may reach a scope by name, or neither its record nor the capture table describes it.
    /// Each function whose record is not built, judged by its captures, joins `unbuilt`.
    fn nested_functions_keep(&self, once: &mut BitSet, unbuilt: &mut Vec<u32>) -> bool {
        let count = self.functions.len();
        let Some(parents) = (0..count).map(|index| self.function_parent(index)).collect::<Option<Vec<_>>>() else {
            return false;
        };
        (1..count).all(|index| {
            let Some(level) = nesting_level(&parents, index) else { return false };
            match self.functions[index].record() {
                Some(function) if function.instructions_of(|op| op == Op::CallFirstChunk).next().is_none() => {
                    if !function.own_names_resolve_statically(self) {
                        return false;
                    }
                    for store in function.instructions_of(|op| op == Op::StoreCapture) {
                        if u32::from(store.capture_depth()) + 1 >= level {
                            once.remove(usize::from(store.capture_slot()));
                        }
                    }
                    true
                }
                // A stub or an unbuilt record: every capture of the root it was laid out for.
                record => self.closure_reads.as_ref().and_then(|reads| reads.captures(index as u32)).is_some_and(|(captures, _)| {
                    if record.is_none() {
                        unbuilt.push(index as u32);
                    }
                    for &(distance, slot) in captures {
                        if distance >= level {
                            once.remove(usize::from(slot));
                        }
                    }
                    true
                }),
            }
        })
    }

    /// Function `index`'s parent, from its record or the capture table; `None` where neither
    /// describes it.
    fn function_parent(&self, index: usize) -> Option<Option<u32>> {
        match self.functions[index].record() {
            Some(function) => Some(function.parent),
            None => self.closure_reads.as_ref()?.captures(index as u32).map(|(_, parent)| parent),
        }
    }
}

/// Function `index`'s nesting level below the root (the root's child is 1), by `parents`;
/// `None` for a chain that does not reach the root.
fn nesting_level(parents: &[Option<u32>], index: usize) -> Option<u32> {
    let mut level = 0;
    let mut current = index;
    while current != 0 {
        current = parents.get(current).copied().flatten()? as usize;
        level += 1;
        if level as usize > parents.len() {
            return None;
        }
    }
    Some(level)
}

/// Notes in `writes` what the root's `instruction` at `pc` writes.
fn note_root_write(
    root: &Function,
    writes: &mut [RootSlotWrites],
    pc: usize,
    instruction: WideInstruction,
    first_guest_code: usize,
    backward: &[(usize, usize)],
) {
    let op = instruction.op();
    match op {
        Op::InitializeTdz => {
            if let Some(slot_writes) = writes.get_mut(instruction.local_slot()) {
                slot_writes.tdz_late |= slot_writes.store.is_some() || may_repeat(backward, pc);
                slot_writes.tdz_before_guest_code |= pc < first_guest_code;
            }
        }
        Op::CloneEnv => {
            for &slot in &root.environment_clones[instruction.environment_clone_index()].fresh {
                note_store(writes, usize::from(slot), pc);
            }
        }
        Op::StoreCapture => note_store(writes, usize::from(instruction.capture_slot()), pc),
        _ if op.immediate_role() == ImmediateRole::LocalSlot && !reads_local_slot(op) => note_store(writes, instruction.local_slot(), pc),
        _ => {}
    }
}

/// Notes a value store to `slot` at `pc`.
fn note_store(writes: &mut [RootSlotWrites], slot: usize, pc: usize) {
    if let Some(slot_writes) = writes.get_mut(slot) {
        slot_writes.store.get_or_insert(pc);
        slot_writes.stores = slot_writes.stores.saturating_add(1);
    }
}
