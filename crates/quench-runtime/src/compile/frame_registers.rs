//! Plain frame locals (`Function::plain_local_slots`) whose every access is a plain load, a
//! plain store, a dead-zone opening or an operand read move into registers of their own. A function left addressing no frame slot runs without a frame in a lane
//! (`CallPlan::registers_only`). A plain slot has no observer but these accesses, its dead
//! zone is never read, and its frame storage is reached only through them, so the register
//! holds exactly what the slot would; opening its dead zone writes nothing anyone reads.

use super::liveness::ROOT_MAP_REGISTERS;
use super::rewrite::{fixed_width, relocate};
/// The binding legacy argument reflection looks for.
const ARGUMENTS_NAME: &str = "arguments";

use crate::bytecode::initialization;
use crate::bytecode::{
    WITH_ENTER_BINDING, EvalBindingLocation, Function, ImmediateRole, Instr, InstructionField, Op,
    Operand, OperandKind, REGISTER_MASK, Register, Superinstruction, ThrownValueSlot, WideInstruction,
};

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
            matches!(instruction.op(), Op::MakeClosure | Op::LoadEnvLocal | Op::StoreEnvLocal | Op::CloneEnv)
        }),
    };
    let named = function.instructions().any(|instruction| {
        if instruction.op().immediate_role() == ImmediateRole::SuperinstructionIndex {
            return superinstructions[instruction.superinstruction_index()]
                .code
                .iter()
                .any(|member| member.is_wide() || member.as_wide().addressed_local_slots().any(|addressed| addressed == slot));
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
                        .code
                        .iter()
                        .map(|member| member.as_wide())
                }),
        )
        .filter(|instruction| instruction.op() == Op::CloneEnv)
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
    let mut by_name = Vec::new();
    for instruction in function.instructions() {
        if instruction.op().immediate_role() == ImmediateRole::SuperinstructionIndex {
            for member in superinstructions[instruction.superinstruction_index()].code {
                if member.is_wide() {
                    return;
                }
                member
                    .as_wide()
                    .addressed_local_slots()
                    .for_each(|slot| exclude(usize::from(slot)));
            }
            continue;
        }
        if matches!(
            instruction.op(),
            Op::LoadName
                | Op::LoadNameCall
                | Op::LoadNameTypeof
                | Op::ResolveName
                | Op::LoadResolvedName
                | Op::DeleteName
                | Op::StoreName
                | Op::StoreResolvedName
        ) {
            by_name.push(instruction.atom_index());
        }
        if let Some(target) = instruction.numeric_local_target() {
            exclude(usize::from(target));
        }
        if instruction.op().immediate_role() != ImmediateRole::LocalSlot {
            continue;
        }
        let slot = instruction.local_slot();
        let promotable = match instruction.op() {
            Op::InitializeTdz => true,
            // The move keeps the load's result flags.
            Op::LoadLocalPlain => {
                instruction.numeric_local_target().is_none()
                    && instruction.numeric_local_store_target().is_none()
                    && WideInstruction::new(Op::Move, instruction.a(), 0, 0, 0).result_flags_valid()
            }
            Op::StoreLocalPlain => true,
            _ => false,
        };
        if !promotable {
            exclude(slot);
        }
    }
    by_name.sort_unstable(); by_name.dedup();
    for (slot, local) in function.local_atoms.iter().enumerate() {
        if by_name.binary_search(local).is_ok() {
            exclude(slot);
        }
    }
    // New registers follow the existing ones while liveness still covers the function and
    // operands can name them. A function whose registers already outnumber what liveness
    // covers has no root map to keep (`ROOT_MAP_REGISTERS`), so only the operand domain
    // bounds it: React SSR's `pushStartInstance` (188 registers) keeps its switch and
    // iteration slots in registers and runs without a frame.
    let uncovered = function.registers >= ROOT_MAP_REGISTERS;
    // A slot no instruction or handler names (a local compilation already promoted) needs no
    // register: nothing reads or writes it, and each register costs every activation a fill.
    let mut accessed = crate::bitset::BitSet::new(eligible.len());
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
    let mut registers: Vec<Option<Register>> = vec![None; eligible.len()];
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
    if next == function.registers {
        return;
    }
    function.registers = next;
    let register = |slot: usize| registers.get(slot).copied().flatten();
    // A frame slot starts undefined, a register only once written (`prepare_registers_for`):
    // a promoted slot some read can reach before any store gets `undefined` on entry.
    let promoted = crate::bitset::BitSet::from_fn(registers.len(), |slot| registers[slot].is_some());
    let written_first = initialization::initialized_before_reads(
        function,
        superinstructions,
        &promoted,
        &promoted,
    );
    let old = function.code.clone();
    let mut code = Vec::with_capacity(old.len());
    for (slot, target) in registers.iter().enumerate() {
        if let Some(target) = *target
            && !written_first.contains(slot)
        {
            let load = WideInstruction::new(Op::LoadConst, target, 0, 0, undefined_constant);
            code.push(encode(load, Instr::new(Op::Nop, 0, 0, 0, 0), &mut function.wide));
        }
    }
    let mut map = vec![0; old.len() + 1];
    for (pc, &packed) in old.iter().enumerate() {
        map[pc] = code.len();
        let mut instruction = fixed_width(packed, &function.wide);
        let promoted = (instruction.op().immediate_role() == ImmediateRole::LocalSlot)
            .then(|| register(instruction.local_slot()))
            .flatten();
        match (instruction.op(), promoted) {
            (Op::InitializeTdz, Some(_)) => continue,
            (Op::LoadLocalPlain, Some(source)) => {
                instruction = WideInstruction::new(Op::Move, instruction.a(), source, 0, 0);
            }
            (Op::StoreLocalPlain, Some(target)) => {
                let source = instruction.register_a();
                // A store that also keeps its value in a register becomes two moves.
                if let Some(copy) = instruction.optional_register_b() {
                    let store = WideInstruction::new(Op::Move, target, source, 0, 0);
                    code.push(encode(store, Instr::new(Op::Nop, 0, 0, 0, 0), &mut function.wide));
                    instruction = WideInstruction::new(Op::Move, copy, source, 0, 0);
                } else {
                    instruction = WideInstruction::new(Op::Move, target, source, 0, 0);
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
        &map,
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
}

/// `instruction` in the slot `packed` held: packed when it fits, else in the wide table,
/// reusing `packed`'s entry when it was wide.
fn encode(instruction: WideInstruction, packed: Instr, wide: &mut Vec<WideInstruction>) -> Instr {
    if packed.is_wide() {
        wide[packed.wide_index()] = instruction;
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
