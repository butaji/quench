//! Portable string-concatenation covering over canonical residual operations.
//!
//! The recipe removes repeated dispatch and local decoding for a bounded chain.
//! It deliberately rejects object coercions before entry; canonical `Add` owns
//! both concatenations once the primitive guards have succeeded.

use crate::ir::Opcode;
use crate::machine::{BaselineEntry, CodeView};
use crate::value::Value;

const REGION_LEN: usize = 6;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LocalSelection {
    slots: [u16; 3],
}

#[derive(Clone, Debug, PartialEq)]
struct ConstantCallSelection {
    callee_slot: u16,
    arguments: [Value; 3],
}

#[derive(Clone, Debug, PartialEq)]
enum StringConcatRecipe {
    Locals(LocalSelection),
    ConstantCall(ConstantCallSelection),
}

pub(crate) struct StringConcatPlan {
    recipe: StringConcatRecipe,
}

pub(crate) struct StringConcatOutcome {
    pub(crate) value: Value,
    pub(crate) constant_call: bool,
}

impl StringConcatPlan {
    pub(crate) const fn span(&self) -> usize {
        REGION_LEN
    }

    pub(crate) fn execute(
        &self,
        environment: &crate::environment::Environment,
    ) -> Result<Option<StringConcatOutcome>, crate::execute::VmError> {
        let Some((inputs, constant_call)) = self.inputs(environment) else {
            return Ok(None);
        };
        let [left, middle, right] = inputs;
        let first = add(&left, &middle)?;
        let value = add(&first, &right)?;
        Ok(Some(StringConcatOutcome {
            value,
            constant_call,
        }))
    }

    fn inputs(&self, environment: &crate::environment::Environment) -> Option<([Value; 3], bool)> {
        match &self.recipe {
            StringConcatRecipe::Locals(selection) => {
                Some((selection.load_inputs(environment)?, false))
            }
            StringConcatRecipe::ConstantCall(selection) => selection
                .load_inputs(environment)
                .map(|values| (values, true)),
        }
    }
}

impl LocalSelection {
    fn load_inputs(self, environment: &crate::environment::Environment) -> Option<[Value; 3]> {
        let values = self
            .slots
            .map(|slot| environment.proven_tagged_bits(slot).and_then(own));
        let [Some(left), Some(middle), Some(right)] = values else {
            return None;
        };
        (is_string(&left) && primitive(&middle) && primitive(&right))
            .then_some([left, middle, right])
    }
}

impl ConstantCallSelection {
    fn load_inputs(&self, environment: &crate::environment::Environment) -> Option<[Value; 3]> {
        let bits = environment.proven_tagged_bits(self.callee_slot)?;
        let Value::Function(function) = own(bits)? else {
            return None;
        };
        string_concat_function(&function).then_some(())?;
        guarded_inputs(self.arguments.clone())
    }
}

fn own(bits: u64) -> Option<Value> {
    crate::register_file::own_tagged_bits(bits)
}

fn is_string(value: &Value) -> bool {
    matches!(value, Value::String(_) | Value::StringUnits(_))
}

fn primitive(value: &Value) -> bool {
    matches!(
        value,
        Value::Number(_)
            | Value::Boolean(_)
            | Value::String(_)
            | Value::StringUnits(_)
            | Value::BigInt(_)
            | Value::Null
            | Value::Undefined
    )
}

fn add(left: &Value, right: &Value) -> Result<Value, crate::execute::VmError> {
    let value = crate::vm::evaluate_binary(left, right, crate::ops::BinaryOp::Add)?;
    #[cfg(test)]
    crate::test_execution_profile::slow("StringConcat");
    Ok(value)
}

pub(crate) fn select_string_concat(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<StringConcatPlan> {
    select_local(entries, cfg, start)
        .map(|selection| StringConcatPlan {
            recipe: StringConcatRecipe::Locals(selection),
        })
        .or_else(|| select_constant_call(code, entries, cfg, start))
}

fn select_local(
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<LocalSelection> {
    let end = start.checked_add(REGION_LEN)?;
    let [a, b, add_ab, c, add_c, ret] = entries.get(start..end)? else {
        return None;
    };
    concat_shape([a, b, add_ab, c, add_c, ret])?;
    cfg.region_control(start, end)?;
    Some(LocalSelection {
        slots: [a.instruction.b, b.instruction.b, c.instruction.b],
    })
}

fn select_constant_call(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<StringConcatPlan> {
    let end = start.checked_add(REGION_LEN)?;
    let [load, a, b, c, call, ret] = entries.get(start..end)? else {
        return None;
    };
    constant_call_shape([load, a, b, c, call, ret], code, start)?;
    cfg.region_control(start, end)?;
    let arguments = [start + 1, start + 2, start + 3]
        .map(|pc| code.constant_at(pc).map(|(_, value)| value.into()))
        .into_iter()
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    Some(StringConcatPlan {
        recipe: StringConcatRecipe::ConstantCall(ConstantCallSelection {
            callee_slot: load.instruction.b,
            arguments,
        }),
    })
}

fn constant_call_shape(
    entries: [&BaselineEntry; REGION_LEN],
    code: CodeView<'_>,
    start: usize,
) -> Option<()> {
    let [load, a, b, c, call, ret] = entries.map(|entry| entry.instruction);
    let loads = load.opcode == Opcode::LoadLocal
        && [a, b, c].iter().all(|op| op.opcode == Opcode::LoadConst);
    let arguments = code.operand_window_at(start + 4)?;
    let call_shape = call.opcode == Opcode::Call
        && call.b == load.a
        && arguments == [a.a, b.a, c.a]
        && ret.opcode == Opcode::Return
        && ret.a == call.a;
    (loads && call_shape).then_some(())
}

fn string_concat_function(function: &crate::value::FunctionValue) -> bool {
    let eligible = function.params == 3
        && !function.is_async
        && matches!(
            function.kind,
            crate::ops::FunctionKind::Ordinary | crate::ops::FunctionKind::Arrow
        );
    eligible
        && function.code.code().is_some_and(|code| {
            code.len() == 8
                && (0..REGION_LEN)
                    .filter_map(|pc| code.instruction(pc))
                    .collect::<Vec<_>>()
                    .as_slice()
                    .try_into()
                    .ok()
                    .and_then(concat_instructions)
                    .is_some()
        })
}

fn concat_instructions(entries: &[crate::ir::Instruction; REGION_LEN]) -> Option<()> {
    let synthetic = entries.map(|instruction| BaselineEntry {
        instruction,
        control: instruction.opcode.control_operands(instruction),
    });
    concat_shape(synthetic.each_ref())
}

fn guarded_inputs(values: [Value; 3]) -> Option<[Value; 3]> {
    (is_string(&values[0]) && primitive(&values[1]) && primitive(&values[2])).then_some(values)
}

fn concat_shape(entries: [&BaselineEntry; REGION_LEN]) -> Option<()> {
    let [a, b, add_ab, c, add_c, ret] = entries.map(|entry| entry.instruction);
    let loads = [a, b, c].iter().all(|op| op.opcode == Opcode::LoadLocal);
    let adds = [add_ab, add_c].iter().all(|op| {
        op.opcode == Opcode::Add
            && op.opcode.binary_operator(op.flags) == Some(crate::ops::BinaryOp::Add)
    });
    let wiring = add_ab.b == a.a
        && add_ab.c == b.a
        && add_c.b == add_ab.a
        && add_c.c == c.a
        && ret.opcode == Opcode::Return
        && ret.a == add_c.a;
    (loads && adds && wiring).then_some(())
}
