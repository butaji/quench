//! Plain frame locals (`Function::plain_local_slots`) whose every access is a plain load, a
//! plain store without a copy, a dead-zone opening or an operand read move into registers of
//! their own. A function left addressing no frame slot runs without a frame in a lane
//! (`CallPlan::registers_only`). A plain slot has no observer but these accesses, its dead
//! zone is never read, and its frame storage is reached only through them, so the register
//! holds exactly what the slot would; opening its dead zone writes nothing anyone reads.

use super::liveness::ROOT_MAP_REGISTERS;
use super::rewrite::{fixed_width, relocate};
use crate::bytecode::{
    EvalBindingLocation, FieldLayout, Function, ImmediateRole, Instr, InstructionField, Op,
    Operand, OperandKind, Register, Superinstruction, WideInstruction,
};

pub(super) fn promote(function: &mut Function, superinstructions: &[Superinstruction]) {
    let mut eligible = function.plain_local_slots(superinstructions);
    if !eligible.contains(&true) {
        return;
    }
    // Parameters arrive in their slots, and these slots are named outside the code: the
    // `arguments` and self bindings, eval binding sites, handlers' completion slots and
    // environment clone plans.
    let parameters = usize::from(function.params).min(eligible.len());
    eligible[..parameters].fill(false);
    let named = function
        .arguments_slot
        .into_iter()
        .chain(function.self_binding_slot)
        .chain(
            function
                .handlers
                .iter()
                .flat_map(|handler| handler.slot.into_iter().chain(handler.return_slot)),
        )
        .chain(
            function
                .environment_clones
                .iter()
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
        if let Some(target) = instruction.numeric_local_target() {
            exclude(usize::from(target));
        }
        if instruction.op().immediate_role() != ImmediateRole::LocalSlot {
            continue;
        }
        let slot = instruction.local_slot();
        let promotable = match instruction.op() {
            Op::InitializeTdz => true,
            Op::LoadLocalPlain => {
                instruction.numeric_local_target().is_none()
                    && instruction.numeric_local_store_target().is_none()
            }
            Op::StoreLocalPlain => instruction.optional_register_b().is_none(),
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
        if eligible[slot] && next < ROOT_MAP_REGISTERS && next <= Operand::PAYLOAD_MASK {
            *register = Some(next);
            next += 1;
        }
    }
    if next == function.registers {
        return;
    }
    function.registers = next;
    let register = |slot: usize| registers.get(slot).copied().flatten();
    let old = function.code.clone();
    let mut code = Vec::with_capacity(old.len());
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
                instruction = WideInstruction::new(Op::Move, instruction.result_register(), source, 0, 0);
            }
            (Op::StoreLocalPlain, Some(target)) => {
                instruction = WideInstruction::new(Op::Move, target, instruction.register_a(), 0, 0);
            }
            _ => {
                let mut changed = false;
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
