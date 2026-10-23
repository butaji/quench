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
    WITH_ENTER_BINDING, EvalBindingLocation, FieldLayout, Function, ImmediateRole, Instr, InstructionField, Op,
    Operand, OperandKind, REGISTER_MASK, Register, Superinstruction, ThrownValueSlot, WideInstruction,
};

/// `plain_slots` are `function`'s plain local slots (`Function::plain_local_slots`);
/// `undefined_constant` is the program's constant index of `undefined`.
pub(super) fn promote<'a>(
    function: &mut Function,
    mut eligible: Vec<bool>,
    superinstructions: &[Superinstruction],
    undefined_constant: u32,
    atom_name: impl Fn(usize) -> Option<&'a str>,
) {
    // A by-name read inside `with` resolves a local through its frame slot.
    if !eligible.contains(&true)
        || function.contains_with_statement(|atom| atom_name(atom) == Some(WITH_ENTER_BINDING))
    {
        return;
    }
    // Legacy `f.arguments` reflection decides from the accesses of a local named
    // `arguments` (`Function::reflects_live_parameters`), which keep their slot.
    for (slot, atom) in function.local_atoms.iter().enumerate() {
        if atom_name(*atom as usize) == Some(ARGUMENTS_NAME) {
            eligible[slot] = false;
        }
    }
    // Parameters arrive in their slots, and these slots are named outside the code: the
    // `arguments` and self bindings, eval binding sites, handlers' return completion slots
    // and the environment clone plans a `CloneEnv` still runs (capture layout drops
    // unobservable clones, leaving their plans unreferenced). A handler's thrown value
    // follows its slot into a register (`ThrownValueSlot::Register`).
    let parameters = usize::from(function.params).min(eligible.len());
    eligible[..parameters].fill(false);
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
        .map(|instruction| instruction.environment_clone_index())
        .collect::<Vec<_>>();
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
        .chain(
            cloned
                .iter()
                .filter_map(|&index| function.environment_clones.get(index))
                .flat_map(|plan| plan.fresh.iter().chain(&plan.reset).copied()),
        )
        .chain(function.binding_sites.iter().flat_map(|site| {
            site.bindings.iter().filter_map(|binding| match binding.location {
                EvalBindingLocation::Local(slot) => Some(slot),
                EvalBindingLocation::Capture { .. } => None,
            })
        }));
    for slot in named {
        if let Some(eligible) = eligible.get_mut(usize::from(slot)) {
            *eligible = false;
        }
    }
    let mut exclude = |slot: usize| {
        if let Some(eligible) = eligible.get_mut(slot) {
            *eligible = false;
        }
    };
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
            let atom = instruction.atom_index();
            for (slot, local) in function.local_atoms.iter().enumerate() {
                if *local == atom {
                    exclude(slot);
                }
            }
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
    // New registers follow the existing ones while liveness still covers the function and
    // operands can name them.
    let mut next = function.registers;
    let mut registers: Vec<Option<Register>> = vec![None; eligible.len()];
    for (slot, register) in registers.iter_mut().enumerate() {
        if eligible[slot]
            && next < ROOT_MAP_REGISTERS
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
    let promoted: Vec<bool> = registers.iter().map(Option::is_some).collect();
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
            && !written_first[slot]
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
                    if !matches!(
                        instruction.op().field_layout(field),
                        FieldLayout::Operand | FieldLayout::NumericIndexOperand
                    ) {
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
