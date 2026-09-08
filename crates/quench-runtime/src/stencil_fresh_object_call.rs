//! Escape-free composition for pure numeric calls over fresh object literals.

use crate::ir::{Instruction, Opcode, Register};
use crate::machine::{BaselineEntry, CodeView};
use crate::ops::{Constant, Op};
use std::collections::BTreeMap;

const MAX_REGION_LEN: usize = 16;
const MAX_OBJECT_FIELDS: usize = 8;
pub(crate) const PROFILE_NAME: &str = "guarded_vector3_dot_return";

#[derive(Clone)]
struct LiteralObject {
    fields: Vec<(crate::value::PropertyName, f64)>,
}

#[derive(Clone)]
pub(crate) struct FreshObjectCallSelection {
    callee_slot: u16,
    objects: [LiteralObject; 2],
    span: u8,
}

pub(crate) struct NativeFreshObjectCallPlan {
    selection: FreshObjectCallSelection,
    fact: Option<crate::function_call_fact::VectorDotReturn>,
}

impl NativeFreshObjectCallPlan {
    pub(crate) const fn new(selection: FreshObjectCallSelection) -> Self {
        Self {
            selection,
            fact: None,
        }
    }

    pub(crate) fn execute(&mut self, environment: &crate::environment::Environment) -> Option<f64> {
        let target = environment.retain_proven_function(self.selection.callee_slot)?;
        let fact = crate::function_call_fact::stable_vector_dot_return(&mut self.fact, &target)?;
        let left = ordered_values(&self.selection.objects[0], &fact.fields)?;
        let right = ordered_values(&self.selection.objects[1], &fact.fields)?;
        let mut context = crate::native_property::NativeVectorDotContext::new(left, right);
        let status = crate::native_property::execute_vector_dot(&mut context);
        context.result(status)
    }

    pub(crate) const fn span(&self) -> usize {
        self.selection.span as usize
    }
}

fn ordered_values(object: &LiteralObject, fields: &[std::rc::Rc<str>; 3]) -> Option<[f64; 3]> {
    let value = |key: &str| {
        object
            .fields
            .iter()
            .rev()
            .find_map(|(name, value)| (name.as_str() == key).then_some(*value))
    };
    Some([value(&fields[0])?, value(&fields[1])?, value(&fields[2])?])
}

pub(crate) fn select_fresh_object_call(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<FreshObjectCallSelection> {
    let callee = entries.get(start)?.instruction;
    (callee.opcode == Opcode::LoadLocal).then_some(())?;
    let mut values = BTreeMap::new();
    let mut objects = BTreeMap::new();
    for pc in start + 1..entries.len().min(start + MAX_REGION_LEN) {
        let instruction = entries[pc].instruction;
        if instruction.opcode == Opcode::Call {
            return finish_selection(code, cfg, start, pc, callee, instruction, &objects);
        }
        collect_definition(code, instruction, pc, &mut values, &mut objects)?;
    }
    None
}

fn collect_definition(
    code: CodeView<'_>,
    instruction: Instruction,
    pc: usize,
    values: &mut BTreeMap<Register, f64>,
    objects: &mut BTreeMap<Register, LiteralObject>,
) -> Option<()> {
    match instruction.opcode {
        Opcode::LoadConst => collect_constant(code, instruction, values),
        Opcode::Unary => collect_unary(instruction, values),
        Opcode::Slow => collect_object(code.cold_at(pc)?, values, objects),
        _ => None,
    }
}

fn collect_constant(
    code: CodeView<'_>,
    instruction: Instruction,
    values: &mut BTreeMap<Register, f64>,
) -> Option<()> {
    let Constant::Number(value) = code.constant(instruction.b)? else {
        return None;
    };
    values.insert(instruction.a, *value);
    Some(())
}

fn collect_unary(instruction: Instruction, values: &mut BTreeMap<Register, f64>) -> Option<()> {
    (crate::ir::compact_unary_operator(instruction.flags) == Some(crate::ops::UnaryOp::Minus))
        .then_some(())?;
    let value = -*values.get(&instruction.b)?;
    values.insert(instruction.a, value);
    Some(())
}

fn collect_object(
    operation: &Op,
    values: &BTreeMap<Register, f64>,
    objects: &mut BTreeMap<Register, LiteralObject>,
) -> Option<()> {
    let Op::MakeObject { dst, properties } = operation else {
        return None;
    };
    (!properties.is_empty() && properties.len() <= MAX_OBJECT_FIELDS).then_some(())?;
    let fields = properties
        .iter()
        .map(|(name, source)| Some((name.clone(), *values.get(source)?)))
        .collect::<Option<Vec<_>>>()?;
    objects.insert(*dst, LiteralObject { fields });
    Some(())
}

fn finish_selection(
    code: CodeView<'_>,
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
    call_pc: usize,
    callee: Instruction,
    call: Instruction,
    objects: &BTreeMap<Register, LiteralObject>,
) -> Option<FreshObjectCallSelection> {
    let ret = code.instruction(call_pc.checked_add(1)?)?;
    (call.flags == 2 && call.b == callee.a && ret == crate::ir::Instruction::ret(call.a))
        .then_some(())?;
    let [left, right] = code.operand_window_at(call_pc)? else {
        return None;
    };
    let end = call_pc.checked_add(2)?;
    cfg.region_control(start, end)?;
    Some(FreshObjectCallSelection {
        callee_slot: callee.b,
        objects: [objects.get(left)?.clone(), objects.get(right)?.clone()],
        span: u8::try_from(end.checked_sub(start)?).ok()?,
    })
}

pub(crate) fn route() -> impl Iterator<Item = &'static str> {
    [
        "GetNQuickened",
        "GetNQuickened",
        "Mul",
        "GetNQuickened",
        "GetNQuickened",
        "Mul",
        "Add",
        "GetNQuickened",
        "GetNQuickened",
        "Mul",
        "Add",
        "Return",
    ]
    .into_iter()
}
