//! Fit checks of a lane: whether a field, operand, or call instruction fits the lane's handlers.
use super::*;

/// Whether the lane may run `instruction` from its view record: every
/// register it names lies inside the register file and carries no flag.
pub(crate) fn fields_fit_in_lane(instruction: WideInstruction, function: &Function) -> bool {
    let registers = function.registers;
    let op = instruction.op();
    let fields = [
        (InstructionField::A, instruction.a()),
        (InstructionField::B, instruction.b()),
        (InstructionField::C, instruction.c()),
    ];
    let numeric_local = instruction.writes_numeric_local()
        && instruction.a() & quench_runtime::bytecode::RETURN_REGISTER == 0
        && instruction.result_register() < function.locals;
    let registers_fit = fields
        .iter()
        .all(|&(field, value)| match op.field_layout(field) {
            FieldLayout::ResultRegister if field == InstructionField::A && numeric_local => true,
            // A lane call reads its return flag from the field (`call_fits_in_lane`), as does
            // the spread of a kept rest view (`call_rest_miss`), whose every path completes a
            // returning call.
            FieldLayout::ResultRegister
                if field == InstructionField::A
                    && (op.traits().has(quench_runtime::bytecode::Traits::FRAME_RETURNABLE)
                        || op.traits().has(quench_runtime::bytecode::Traits::REST_SPREAD_CALL)
                        || (quench_runtime::staged::computes_binary(op) && instruction.returns_from_frame())) =>
            {
                instruction.result_register() < registers
            }
            FieldLayout::FieldBase => {
                value == quench_runtime::bytecode::FieldBase::NESTED
                    || quench_runtime::bytecode::FieldBase(value)
                        .register_index()
                        .is_none_or(|register| register < registers)
            }
            FieldLayout::OptionalRegister | FieldLayout::OptionalReadRegister => value
                .checked_sub(quench_runtime::bytecode::OPTIONAL_REGISTER_BIAS)
                .is_none_or(|register| register < registers),
            FieldLayout::Operand => {
                let operand = quench_runtime::bytecode::Operand(value);
                operand.kind() != Some(quench_runtime::bytecode::OperandKind::Register)
                    || operand.payload() < registers
            }
            layout => !layout.is_register_field() || value < registers,
        });
    let window_fits = op.field_layout(InstructionField::B) != FieldLayout::RegisterWindowBase
        || u32::from(instruction.b()) + u32::from(instruction.c()) <= u32::from(registers);
    let pair_fits = op.immediate_layout() != ImmediateLayout::RegisterPair || {
        let (first, second) = ImmediateLayout::register_pair(instruction.imm());
        first < registers && second < registers
    };
    registers_fit && window_fits && pair_fits
}

/// The record operand of `instruction` at `pc`, or `None` when its jump or
/// constant cannot be resolved.
pub(crate) fn operand_in_lane(
    instruction: WideInstruction,
    pc: usize,
    constants: &[Value],
) -> Option<LaneOperand> {
    let record = std::mem::size_of::<LaneInstruction>() as isize;
    Some(match instruction.op().immediate_role() {
        ImmediateRole::JumpTarget => {
            let distance = isize::try_from(instruction.imm()).ok()? - isize::try_from(pc).ok()?;
            LaneOperand {
                jump: distance.checked_mul(record)?,
            }
        }
        // A case selector reads its run of constants from the context by index, as a constant
        // array reads its elements' (the `MakeConstArray` row).
        ImmediateRole::ConstantIndex
            if quench_runtime::staged::selects_string_case(instruction.op()) || quench_runtime::staged::const_array_family(instruction.op()) =>
        {
            let start = instruction.imm() as usize;
            constants.get(start..start.checked_add(usize::from(instruction.b()))?)?;
            LaneOperand { imm: instruction.imm() }
        }
        ImmediateRole::ConstantIndex => LaneOperand {
            constant: *constants.get(instruction.imm() as usize)?,
        },
        _ if holds_field_hint(instruction.op()) => LaneOperand {
            field: FieldOperand { imm: instruction.imm(), shape: quench_runtime::vm::NO_FIELD_RECEIVER },
        },
        _ if instruction.op().makes_script_call() => LaneOperand {
            call: CallOperand { imm: instruction.imm(), memo: NO_READ_MEMO },
        },
        _ => LaneOperand {
            imm: instruction.imm(),
        },
    })
}

/// A `Call` runs in the lane only as the general loop pushes one in place: it evaluates no
/// direct or parameter-scope `eval`, which a window activation's call record runs apart (`lane_eval`). It, `GetField`, `Unary` and `Binary` may return their
/// result from the frame (`returns_in_lane`, `returning_miss`); a returning record runs no
/// other result flag.
pub(in super::super) fn call_fits_in_lane(instruction: WideInstruction) -> bool {
    match instruction.op() {
        _ if quench_runtime::staged::call_family(instruction.op()) || quench_runtime::staged::call_array_family(instruction.op())
            || quench_runtime::staged::call_spread_family(instruction.op()) => {
            returns_in_lane(instruction)
                && !instruction.direct_eval()
                && !instruction.parameter_eval()
        }
        _ if quench_runtime::staged::forwarded_call_family(instruction.op()) => returns_in_lane(instruction),
        // `super(...)` takes either argument form (`derived_frames::super_construct_in_frame`).
        _ if quench_runtime::staged::construction_family(instruction.op()) => {
            returns_in_lane(instruction)
                && (instruction.is_super_construct() || !ImmediateLayout::construct_array_arguments(instruction.imm()))
        }
        _ if quench_runtime::staged::field_read_family(instruction.op()) => returns_in_lane(instruction) || sinks_field_in_lane(instruction),
        _ if quench_runtime::staged::unary_family(instruction.op()) || quench_runtime::staged::object_pair_family(instruction.op())
            || quench_runtime::staged::super_const_array_family(instruction.op()) || quench_runtime::staged::object_literal_family(instruction.op()) => {
            returns_in_lane(instruction)
        }
        _ if quench_runtime::staged::binary_family(instruction.op()) && instruction.returns_from_frame() => returns_in_lane(instruction),
        _ if spreads_rest_view(instruction) => returns_in_lane(instruction),
        _ => true,
    }
}

/// Whether `instruction`'s staged row is a call spreading a rest parameter kept as a view
/// (`Passing::Rest`): the interpreter's own decode names it, so no opcode is listed here.
pub(in super::super) fn spreads_rest_view(instruction: WideInstruction) -> bool {
    matches!(quench_runtime::staged::Operation::<u64, ()>::decode_names(instruction),
        Some(quench_runtime::staged::Operation::SpreadCall { passing: quench_runtime::staged::Passing::Rest, .. }))
}
