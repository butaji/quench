use super::ResidualProgram;
use super::instruction::WideInstruction;
use super::{
    FieldLayout, ImmediateLayout, ImmediateRole, InstructionField, Op, Operand, OperandKind,
    Register, ResultLayout,
};
use std::fmt::{self, Write};

impl ResidualProgram {
    pub fn disassemble(&self) -> String {
        let mut output = String::new();
        for (id, function) in self.functions.iter().enumerate() {
            let name = function
                .name
                .map(|atom| self.atoms[atom as usize].as_ref())
                .unwrap_or("<anonymous>");
            let _ = writeln!(
                output,
                "function {id} {name} params={} locals={} registers={} dispatch={:?}",
                function.params, function.locals, function.registers, function.dispatch
            );
            for (pc, packed) in function.code.iter().enumerate() {
                let instruction = if packed.is_wide() {
                    function.wide.get(packed.wide_index()).copied()
                } else {
                    Some(packed.as_wide())
                };
                let Some(instruction) = instruction else {
                    let _ = writeln!(output, "  {pc:04} <invalid wide instruction>");
                    continue;
                };
                let _ = write!(output, "  {pc:04} ");
                let _ = write_instruction(&mut output, instruction);
                let _ = writeln!(output);
            }
        }
        for (id, site) in self.superinstructions.iter().enumerate() {
            let _ = writeln!(output, "superinstruction {id}");
            for instruction in site.code {
                let _ = write!(output, "  ");
                let _ = write_instruction(&mut output, instruction.as_wide());
                let _ = writeln!(output);
            }
        }
        output
    }
}

fn write_instruction(output: &mut String, instruction: WideInstruction) -> fmt::Result {
    write!(output, "{:?}", instruction.op())?;
    write_result(output, instruction)?;
    for field in [
        InstructionField::A,
        InstructionField::B,
        InstructionField::C,
    ] {
        write_field(output, instruction, field)?;
    }
    write_immediate(output, instruction)
}

fn write_result(output: &mut String, instruction: WideInstruction) -> fmt::Result {
    if instruction.op().result_layout() == ResultLayout::NoResult {
        return Ok(());
    }
    write!(output, " result={}", instruction.result_register())?;
    if instruction.returns_from_frame() {
        write!(output, " return")?;
    }
    if instruction.writes_current_this() {
        write!(output, " set-this")?;
    }
    if instruction.writes_numeric_local() {
        write!(output, " numeric-local")?;
    }
    Ok(())
}

fn write_field(
    output: &mut String,
    instruction: WideInstruction,
    field: InstructionField,
) -> fmt::Result {
    let layout = instruction.op().field_layout(field);
    if skip_field(instruction, field, layout) {
        return Ok(());
    }
    match layout {
        FieldLayout::Register
        | FieldLayout::WriteRegister
        | FieldLayout::ReadWriteRegister
        | FieldLayout::OptionalRegister
        | FieldLayout::FunctionIndex
        | FieldLayout::ElementCount
        | FieldLayout::CacheSiteIndex
        | FieldLayout::BooleanFlag => write_scalar_field(output, instruction, field, layout),
        FieldLayout::Operand => write_operand_field(output, instruction, field),
        FieldLayout::BinaryOperator => {
            write!(output, " operator={}", instruction.binary_operator_field())
        }
        FieldLayout::NumericLocalTarget => {
            write!(
                output,
                " local-update={:?}",
                instruction.numeric_local_store_target()
            )
        }
        FieldLayout::Undeclared => write!(
            output,
            " {:?}={}",
            field_name(field),
            field_value(instruction, field)
        ),
        FieldLayout::Unused
        | FieldLayout::NumericLocalStoreMarker
        | FieldLayout::ConstructArguments
        | FieldLayout::FieldBase => Ok(()),
    }
}

fn skip_field(instruction: WideInstruction, field: InstructionField, layout: FieldLayout) -> bool {
    layout == FieldLayout::Unused
        || (field == InstructionField::A
            && instruction.op().result_layout() != ResultLayout::NoResult)
        || (instruction.op() == Op::GetField
            && matches!(field, InstructionField::B | InstructionField::C))
        || (instruction.op() == Op::LoadLocal && field == InstructionField::C)
}

fn write_scalar_field(
    output: &mut String,
    instruction: WideInstruction,
    field: InstructionField,
    layout: FieldLayout,
) -> fmt::Result {
    match (field, layout) {
        (
            InstructionField::A,
            FieldLayout::Register | FieldLayout::WriteRegister | FieldLayout::ReadWriteRegister,
        ) => {
            write!(output, " a={}", instruction.register_a())
        }
        (
            InstructionField::B,
            FieldLayout::Register | FieldLayout::WriteRegister | FieldLayout::ReadWriteRegister,
        ) => {
            write!(output, " b={}", instruction.register_b())
        }
        (
            InstructionField::C,
            FieldLayout::Register | FieldLayout::WriteRegister | FieldLayout::ReadWriteRegister,
        ) => {
            write!(output, " c={}", instruction.register_c())
        }
        (InstructionField::B, FieldLayout::OptionalRegister) => {
            write!(output, " b={:?}", instruction.optional_register_b())
        }
        (InstructionField::B, FieldLayout::FunctionIndex) => {
            write!(output, " b=function:{}", instruction.known_function_index())
        }
        (InstructionField::B, FieldLayout::ElementCount) => {
            write!(output, " elements={}", instruction.element_count())
        }
        (InstructionField::C, FieldLayout::CacheSiteIndex) => {
            write!(output, " cache={}", instruction.cache_site_index())
        }
        (field, FieldLayout::BooleanFlag) => {
            write!(
                output,
                " {}={:?}",
                field_name(field),
                instruction.boolean_field(field)
            )
        }
        _ => Ok(()),
    }
}

fn write_operand_field(
    output: &mut String,
    instruction: WideInstruction,
    field: InstructionField,
) -> fmt::Result {
    match field {
        InstructionField::B => write!(output, " b={}", DisplayOperand(instruction.operand_b())),
        InstructionField::C => write!(output, " c={}", DisplayOperand(instruction.operand_c())),
        InstructionField::A => Ok(()),
    }
}

fn write_immediate(output: &mut String, instruction: WideInstruction) -> fmt::Result {
    match instruction.op().immediate_layout() {
        ImmediateLayout::CaptureDepthAndSlot => write!(
            output,
            " capture={}:{}",
            instruction.capture_depth(),
            instruction.capture_slot()
        ),
        ImmediateLayout::CallWindow
        | ImmediateLayout::CallWindowWithEvalFlags
        | ImmediateLayout::SingleArgumentCallWindowWithEvalFlags => {
            let window = instruction.call_window();
            write!(output, " args={}+{}", window.base, window.count)?;
            if matches!(
                instruction.op().immediate_layout(),
                ImmediateLayout::CallWindowWithEvalFlags
                    | ImmediateLayout::SingleArgumentCallWindowWithEvalFlags
            ) {
                write!(
                    output,
                    " direct-eval={} parameter-eval={}",
                    instruction.direct_eval(),
                    instruction.parameter_eval()
                )?;
            }
            Ok(())
        }
        ImmediateLayout::ConstructCountAndFlags => {
            write!(output, " construct={:?}", instruction.construct_arguments())?;
            write!(output, " super={}", instruction.is_super_construct())
        }
        ImmediateLayout::RegisterPair => {
            let (first, second) = instruction.register_pair();
            write!(output, " registers={first},{second}")
        }
        ImmediateLayout::Scalar => write_scalar_immediate(output, instruction),
    }
}

fn write_scalar_immediate(output: &mut String, instruction: WideInstruction) -> fmt::Result {
    match instruction.op().immediate_role() {
        ImmediateRole::Unused => Ok(()),
        ImmediateRole::Undeclared => write!(output, " imm={}", instruction.imm()),
        ImmediateRole::FieldLookup => write!(output, " lookup={:?}", instruction.field_lookup()),
        ImmediateRole::ConstantIndex
        | ImmediateRole::ClosureFunctionIndex
        | ImmediateRole::AtomIndex
        | ImmediateRole::LocalSlot
        | ImmediateRole::TemplateSiteIndex
        | ImmediateRole::MethodSiteIndex
        | ImmediateRole::ObjectSiteIndex
        | ImmediateRole::SuperinstructionIndex => write_index_immediate(output, instruction),
        ImmediateRole::ArrayLength
        | ImmediateRole::FunctionNamePrefix
        | ImmediateRole::BooleanFlag
        | ImmediateRole::ArrayIndex
        | ImmediateRole::BinaryOperator
        | ImmediateRole::AdditionOperator
        | ImmediateRole::MultiplicationOperator
        | ImmediateRole::UnaryOperator
        | ImmediateRole::JumpTarget => write_scalar_value(output, instruction),
    }
}

fn write_index_immediate(output: &mut String, instruction: WideInstruction) -> fmt::Result {
    match instruction.op().immediate_role() {
        ImmediateRole::ConstantIndex => {
            write!(output, " constant={}", instruction.constant_index())
        }
        ImmediateRole::ClosureFunctionIndex => {
            write!(output, " function={}", instruction.closure_function_index())
        }
        ImmediateRole::AtomIndex => write!(output, " atom={}", instruction.atom_index()),
        ImmediateRole::LocalSlot => write!(output, " local={}", instruction.local_slot()),
        ImmediateRole::TemplateSiteIndex => {
            write!(
                output,
                " template-site={}",
                instruction.template_site_index()
            )
        }
        ImmediateRole::MethodSiteIndex => {
            write!(output, " method-site={}", instruction.method_site_index())
        }
        ImmediateRole::ObjectSiteIndex => {
            write!(output, " object-site={}", instruction.object_site_index())
        }
        ImmediateRole::SuperinstructionIndex => {
            write!(
                output,
                " superinstruction={}",
                instruction.superinstruction_index()
            )
        }
        _ => Ok(()),
    }
}

fn write_scalar_value(output: &mut String, instruction: WideInstruction) -> fmt::Result {
    match instruction.op().immediate_role() {
        ImmediateRole::ArrayLength => write!(output, " length={}", instruction.array_length()),
        ImmediateRole::FunctionNamePrefix => {
            write!(
                output,
                " name-prefix={}",
                instruction.function_name_prefix()
            )
        }
        ImmediateRole::BooleanFlag => write!(output, " flag={:?}", instruction.boolean_flag()),
        ImmediateRole::ArrayIndex => write!(output, " index={}", instruction.array_index()),
        ImmediateRole::BinaryOperator
        | ImmediateRole::AdditionOperator
        | ImmediateRole::MultiplicationOperator => {
            write!(output, " operator={}", instruction.binary_operator())
        }
        ImmediateRole::UnaryOperator => {
            write!(output, " operator={}", instruction.unary_operator())
        }
        ImmediateRole::JumpTarget => write!(output, " target={}", instruction.jump_target()),
        _ => Ok(()),
    }
}

fn field_name(field: InstructionField) -> &'static str {
    match field {
        InstructionField::A => "a",
        InstructionField::B => "b",
        InstructionField::C => "c",
    }
}

fn field_value(instruction: WideInstruction, field: InstructionField) -> Register {
    match field {
        InstructionField::A => instruction.a(),
        InstructionField::B => instruction.b(),
        InstructionField::C => instruction.c(),
    }
}

struct DisplayOperand(Operand);

impl fmt::Display for DisplayOperand {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        let operand = self.0;
        match operand.kind() {
            Some(OperandKind::Register) => write!(output, "r{}", operand.payload()),
            Some(OperandKind::Constant) => write!(output, "constant:{}", operand.payload()),
            Some(OperandKind::Field) => write!(output, "field:{}", operand.payload()),
            Some(OperandKind::Local) => write!(output, "local:{}", operand.payload()),
            None => write!(output, "invalid:{}", operand.0),
        }
    }
}
