//! Plain frame locals (`Function::plain_local_slots`) whose every access is a plain load, a
//! plain store, a dead-zone opening or an operand read move into registers of their own. A function left addressing no frame slot runs without a frame in a lane
//! (`CallPlan::registers_only`). A plain slot has no observer but these accesses, its dead
//! zone is never read, and its frame storage is reached only through them, so the register
//! holds exactly what the slot would; opening its dead zone writes nothing anyone reads.

use super::liveness::ROOT_MAP_REGISTERS;
use super::rewrite::relocate;
use super::CompileScratch;
/// The binding legacy argument reflection looks for.
const ARGUMENTS_NAME: &str = "arguments";

use crate::bytecode::initialization;
use crate::bytecode::{
    REGISTER_MOVE, WITH_ENTER_BINDING, EvalBindingLocation, Function, ImmediateRole, Instr, InstructionField, Op,
    Operand, OperandKind, REGISTER_MASK, Superinstruction, ThrownValueSlot, Traits, WideInstruction, the_op,
};

const LOAD_CONST_OP: Op = the_op(Traits::CONSTANT_LOAD);
const NOP_OP: Op = the_op(Traits::NO_OP);
/// The ops that reach a closure's or an environment's slots, which a frame's own accesses do not see.
const ENVIRONMENT_REACHING: Traits = Traits::MAKES_CLOSURE
    .union(Traits::ENV_LOCAL_LOAD)
    .union(Traits::ENV_LOCAL_STORE)
    .union(Traits::ENVIRONMENT_CLONE);

/// The by-name accesses of an identifier: a name operation whose atom immediate names the identifier.
/// `NAME_OPERATION` alone also covers operations naming no atom, which the immediate role tells apart.
fn names_binding(op: Op) -> bool {
    op.traits().has(Traits::NAME_OPERATION) && op.immediate_role() == ImmediateRole::AtomIndex
}

/// Forgets `function`'s own name binding slot (`Function::self_binding_slot`) when nothing
/// can read it: its body reads the name as `LoadCallee`, no instruction or handler names the
/// slot, no closure's environment can hold it, and no direct eval or `with` resolves names
/// through the frame. Its activation then has no slot to fill with the callee, and a body
/// that addresses no other local runs without a frame (`CallPlan::registers_only`), as
/// Node's `Buffer.from` (`function from(value, ...)`) and Zod's `parse: function _parse()`.
pub(super) fn drop_unread_self_binding<'a>(
    function: &mut Function,
    superinstructions: &[Superinstruction],
    atom_name: impl Fn(usize) -> Option<&'a str>,
) {
    let Some(slot) = function.self_binding_slot else {
        return;
    };
    if function.is_generator
        || function.is_async
        || !function.plain_local_context_is_safe()
        || function.contains_with_statement(|atom| atom_name(atom) == Some(WITH_ENTER_BINDING))
    {
        return;
    }
    let captured = match &function.selective_capture_slots {
        Some(captured) => captured.binary_search(&slot).is_ok(),
        // Without a selective layout a closure or environment access may reach any slot.
        None => function.instructions().any(|instruction| {
            instruction.op().traits().has(ENVIRONMENT_REACHING)
        }),
    };
    let named = function.instructions().any(|instruction| {
        if instruction.op().immediate_role() == ImmediateRole::SuperinstructionIndex {
            return superinstructions[instruction.superinstruction_index()]
                .wide
                .iter()
                .any(|member| member.addressed_local_slots().any(|addressed| addressed == slot));
        }
        instruction.addressed_local_slots().any(|addressed| addressed == slot)
    }) || function.handlers.iter().any(|handler| {
        matches!(handler.slot, Some(ThrownValueSlot::Local(local)) if local == slot) || handler.return_slot == Some(slot)
    }) || function.environment_clones.mentions(slot)
        || function.binding_sites.iter().any(|site| {
            site.bindings.iter().any(|binding| matches!(binding.location, EvalBindingLocation::Local(local) if local == slot))
        });
    if !captured && !named {
        function.self_binding_slot = None;
    }
}

/// `plain_slots` are `function`'s plain local slots (`Function::plain_local_slots`);
/// `undefined_constant` is the program's constant index of `undefined`.
pub(super) fn promote<'a>(
    function: &mut Function,
    mut eligible: crate::bitset::BitSet,
    superinstructions: &[Superinstruction],
    undefined_constant: u32,
    atom_name: impl Fn(usize) -> Option<&'a str>,
    flow: &initialization::FlowCell,
    scratch: &mut CompileScratch,
) {
    // A by-name read inside `with` resolves a local through its frame slot.
    if eligible.is_empty()
        || function.contains_with_statement(|atom| atom_name(atom) == Some(WITH_ENTER_BINDING))
    {
        return;
    }
    // Legacy `f.arguments` reflection decides from the accesses of a local named
    // `arguments` (`Function::reflects_live_parameters`), which keep their slot.
    for (slot, atom) in function.local_atoms.iter().enumerate() {
        if atom_name(*atom as usize) == Some(ARGUMENTS_NAME) {
            eligible.remove(slot);
        }
    }
    // Parameters arrive in their slots, and these slots are named outside the code: the
    // `arguments` and self bindings, eval binding sites, handlers' return completion slots
    // and the environment clone plans a `CloneEnv` still runs (capture layout drops
    // unobservable clones, leaving their plans unreferenced). A handler's thrown value
    // follows its slot into a register (`ThrownValueSlot::Register`).
    let parameters = usize::from(function.params).min(eligible.len());
    (0..parameters).for_each(|slot| eligible.remove(slot));
    let cloned = function
        .instructions()
        .chain(
            function
                .instructions()
                .filter(|instruction| {
                    instruction.op().immediate_role() == ImmediateRole::SuperinstructionIndex
                })
                .flat_map(|instruction| {
                    superinstructions[instruction.superinstruction_index()]
                        .wide
                        .iter()
                        .copied()
                }),
        )
        .filter(|instruction| instruction.op().traits().has(Traits::ENVIRONMENT_CLONE))
        .map(|instruction| instruction.environment_clone_index());
    let named = function
        .arguments_slot
        .into_iter()
        .chain(function.self_binding_slot)
        .chain(
            function
                .handlers
                .iter()
                .filter_map(|handler| handler.return_slot),
        )
        .chain(function.environment_clones.named_slots(cloned.into_iter()))
        .chain(function.binding_sites.iter().flat_map(|site| {
            site.bindings.iter().filter_map(|binding| match binding.location {
                EvalBindingLocation::Local(slot) => Some(slot),
                EvalBindingLocation::Capture { .. } => None,
            })
        }));
    for slot in named {
        eligible.remove(usize::from(slot));
    }
    let mut exclude = |slot: usize| eligible.remove(slot);
    // The atoms by-name accesses resolve; a local of one of them keeps its slot.
    let mut by_name = std::mem::take(&mut scratch.by_name);
    by_name.clear();
    for instruction in function.instructions() {
        if instruction.op().immediate_role() == ImmediateRole::SuperinstructionIndex {
            for member in superinstructions[instruction.superinstruction_index()].wide {
                member
                    .addressed_local_slots()
                    .for_each(|slot| exclude(usize::from(slot)));
            }
            continue;
        }
        if names_binding(instruction.op()) {
            by_name.push(instruction.atom_index());
        }
        if let Some(target) = instruction.numeric_local_target() {
            exclude(usize::from(target));
        }
        if instruction.op().immediate_role() != ImmediateRole::LocalSlot {
            continue;
        }
        let slot = instruction.local_slot();
        let op = instruction.op();
        let promotable = if op.traits().has(Traits::TDZ_INITIALIZE) {
            true
        // The move keeps the load's result flags.
        } else if op.traits().has(Traits::PLAIN_LOCAL_LOAD) {
            instruction.numeric_local_target().is_none()
                && instruction.numeric_local_store_target().is_none()
                && WideInstruction::new(REGISTER_MOVE, instruction.a(), 0, 0, 0).result_flags_valid()
        } else {
            op.traits().has(Traits::PLAIN_LOCAL_STORE)
        };
        if !promotable {
            exclude(slot);
        }
    }
    by_name.sort_unstable(); by_name.dedup();
    function.local_atoms.iter().enumerate().filter(|(_, local)| by_name.binary_search(local).is_ok()).for_each(|(slot, _)| exclude(slot));
    scratch.by_name = by_name;
    // New registers follow the existing ones while liveness still covers the function and
    // operands can name them. A function whose registers already outnumber what liveness
    // covers has no root map to keep (`ROOT_MAP_REGISTERS`), so only the operand domain
    // bounds it: React SSR's `pushStartInstance` (188 registers) keeps its switch and
    // iteration slots in registers and runs without a frame.
    let uncovered = function.registers >= ROOT_MAP_REGISTERS;
    // A slot no instruction or handler names (a local compilation already promoted) needs no
    // register: nothing reads or writes it, and each register costs every activation a fill.
    let mut accessed = std::mem::take(&mut scratch.accessed);
    accessed.reset(eligible.len());
    let mut access = |slot: usize| {
        accessed.checked_insert(slot);
    };
    for instruction in function.instructions() {
        instruction.addressed_local_slots().for_each(|slot| access(usize::from(slot)));
    }
    for handler in &function.handlers {
        if let Some(ThrownValueSlot::Local(slot)) = handler.slot {
            access(usize::from(slot));
        }
    }
    let mut next = function.registers;
    let mut registers = std::mem::take(&mut scratch.registers);
    registers.clear();
    registers.resize(eligible.len(), None);
    for (slot, register) in registers.iter_mut().enumerate() {
        if eligible.contains(slot)
            && accessed.contains(slot)
            && (uncovered || next < ROOT_MAP_REGISTERS)
            && next <= Operand::PAYLOAD_MASK
            && next <= REGISTER_MASK
        {
            *register = Some(next);
            next += 1;
        }
    }
    scratch.accessed = accessed;
    if next == function.registers {
        scratch.registers = registers;
        return;
    }
    function.registers = next;
    let register = |slot: usize| registers.get(slot).copied().flatten();
    // A frame slot starts undefined, a register only once written (`prepare_registers_for`):
    // a promoted slot some read can reach before any store gets `undefined` on entry.
    let promoted = crate::bitset::BitSet::from_fn(registers.len(), |slot| registers[slot].is_some());
    let written_first = initialization::initialized_before_reads(function, superinstructions, &promoted, &promoted, flow);
    // The code is moved out, not copied: nothing below reads `function.code` before it is set back.
    let old = std::mem::take(&mut function.code);
    let mut code = scratch.code_buffer(old.len());
    for (slot, target) in registers.iter().enumerate() {
        if let Some(target) = *target
            && !written_first.contains(slot)
        {
            let load = WideInstruction::new(LOAD_CONST_OP, target, 0, 0, undefined_constant);
            code.push(encode(load, Instr::new(NOP_OP, 0, 0, 0, 0), &mut function.wide));
        }
    }
    let map = scratch.remap(old.len());
    for (pc, &packed) in old.iter().enumerate() {
        map[pc] = code.len();
        let mut instruction = function.widened(packed);
        let promoted = (instruction.op().immediate_role() == ImmediateRole::LocalSlot)
            .then(|| register(instruction.local_slot()))
            .flatten();
        match (instruction.op(), promoted) {
            (op, Some(_)) if op.traits().has(Traits::TDZ_INITIALIZE) => continue,
            (op, Some(source)) if op.traits().has(Traits::PLAIN_LOCAL_LOAD) => {
                instruction = WideInstruction::new(REGISTER_MOVE, instruction.a(), source, 0, 0);
            }
            (op, Some(target)) if op.traits().has(Traits::PLAIN_LOCAL_STORE) => {
                let source = instruction.register_a();
                // A store that also keeps its value in a register becomes two moves. The copy
                // reads the local, not the stored value, so the value's register is dead after
                // the store and its producer can compute straight into the local
                // (`coalesce_distant_producer_moves`); copy propagation then lets the copy's
                // reader read the local (`forward_move_sources`). A copy into the stored
                // value's own register already holds the value and needs no move.
                if let Some(copy) = instruction.optional_register_b().filter(|&copy| copy != source) {
                    let store = WideInstruction::new(REGISTER_MOVE, target, source, 0, 0);
                    code.push(encode(store, Instr::new(NOP_OP, 0, 0, 0, 0), &mut function.wide));
                    instruction = WideInstruction::new(REGISTER_MOVE, copy, target, 0, 0);
                } else {
                    instruction = WideInstruction::new(REGISTER_MOVE, target, source, 0, 0);
                }
            }
            _ => {
                let mut changed = false;
                // A field read fused onto a local (`rewrite::LocalField`) reads the register
                // the local now lives in, as the load it absorbed would have.
                if let Some(target) = instruction.local_field_base().and_then(|local| register(usize::from(local))) {
                    instruction.set_b(crate::bytecode::FieldBase::register(target).0);
                    changed = true;
                }
                for field in [InstructionField::B, InstructionField::C] {
                    if !instruction.op().field_layout(field).holds_operand() {
                        continue;
                    }
                    let operand = Operand(instruction.field_value(field));
                    if operand.kind() == Some(OperandKind::Local)
                        && let Some(target) = register(usize::from(operand.payload()))
                    {
                        let operand = Operand::register(target);
                        if field == InstructionField::B {
                            instruction.set_operand_b(operand);
                        } else {
                            instruction.set_operand_c(operand);
                        }
                        changed = true;
                    }
                }
                if !changed {
                    code.push(packed);
                    continue;
                }
            }
        }
        code.push(encode(instruction, packed, &mut function.wide));
    }
    map[old.len()] = code.len();
    relocate(
        &mut code,
        &mut function.wide,
        &*map,
        &mut function.handlers,
        &mut function.parameter_end_pc,
        &mut function.binding_sites,
        &mut function.source_positions,
        &old,
    );
    for handler in function.handlers.iter_mut() {
        if let Some(ThrownValueSlot::Local(slot)) = handler.slot
            && let Some(target) = register(usize::from(slot))
        {
            handler.slot = Some(ThrownValueSlot::Register(target));
        }
    }
    function.code = code;
    scratch.recycle_code(old);
    scratch.registers = registers;
}

/// `instruction` in the slot `packed` held: packed when it fits, else in the wide table,
/// reusing `packed`'s entry when it was wide.
fn encode(instruction: WideInstruction, packed: Instr, wide: &mut Vec<WideInstruction>) -> Instr {
    if packed.is_wide() {
        *crate::bytecode::wide_entry_mut(wide, packed) = instruction;
        return packed;
    }
    Instr::try_new(
        instruction.op(),
        instruction.a(),
        instruction.b(),
        instruction.c(),
        instruction.imm(),
    )
    .unwrap_or_else(|| {
        wide.push(instruction);
        Instr::wide(wide.len() - 1).expect("wide table index fits the packed domain")
    })
}

#[cfg(test)]
mod role_tests {
    use super::*;

    /// The catalog's ops, in row order, that a predicate selects.
    fn selected(predicate: impl Fn(Op) -> bool) -> Vec<Op> {
        (0..Op::COUNT).filter_map(Op::from_index).filter(|&op| predicate(op)).collect()
    }

    /// Each role selects exactly the ops its emitters and matches replaced.
    #[test]
    fn roles_select_the_ops_they_replaced() {
        assert_eq!(selected(|op| op.traits().has(Traits::LOCAL_BINDING_LOAD)), [Op::LoadLocal]);
        assert_eq!(selected(|op| op.traits().has(Traits::LOCAL_BINDING_STORE)), [Op::StoreLocal]);
        assert_eq!(selected(|op| op.traits().has(Traits::PLAIN_LOCAL_LOAD)), [Op::LoadLocalPlain]);
        assert_eq!(selected(|op| op.traits().has(Traits::PLAIN_LOCAL_STORE)), [Op::StoreLocalPlain]);
        assert_eq!(selected(|op| op.traits().has(Traits::INCREMENT_UPDATE)), [Op::IncDec]);
        assert_eq!(selected(|op| op.traits().has(Traits::BINARY_RESULT)), [Op::Binary]);
        assert_eq!(selected(|op| op.traits().has(Traits::PAIR_LITERAL_MAKE)), [Op::MakeObject2]);
        assert_eq!(selected(|op| op.traits().has(Traits::WINDOW_LITERAL_MAKE)), [Op::MakeObjectLiteral]);
        assert_eq!(selected(|op| op.traits().has(Traits::NAMED_FIELD_WRITE)), [Op::SetField, Op::SetFieldStrict]);
        assert_eq!(
            selected(|op| op.traits().has(Traits::FRAME_SLOT_READ)),
            [Op::LoadLocal, Op::ArgumentsLength, Op::ArgumentGet],
        );
        assert_eq!(
            selected(|op| op.traits().has(ENVIRONMENT_REACHING)),
            [Op::CloneEnv, Op::LoadEnvLocal, Op::StoreEnvLocal, Op::MakeClosure],
        );
        assert_eq!(
            selected(names_binding),
            [
                Op::LoadName, Op::LoadNameCall, Op::LoadNameTypeof, Op::ResolveName,
                Op::LoadResolvedName, Op::DeleteName, Op::StoreName, Op::StoreResolvedName,
            ],
        );
        assert_eq!(selected(|op| op.traits().has(Traits::CONSTANT_LOAD)), [Op::LoadConst]);
        assert_eq!(selected(|op| op.traits().has(Traits::NO_OP)), [Op::Nop]);
        assert_eq!(selected(|op| op.traits().has(Traits::TDZ_INITIALIZE)), [Op::InitializeTdz]);
        assert_eq!(selected(|op| op.traits().has(Traits::REGISTER_COPY)), [Op::Move]);
    }
}
