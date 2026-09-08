//! Escape-free composition for pure numeric calls over fresh object literals.

use crate::ir::{Instruction, Opcode, Register};
use crate::machine::{BaselineEntry, CodeView};
use crate::ops::{Constant, Op};
use std::collections::BTreeMap;

const MAX_REGION_LEN: usize = 16;
const MAX_OBJECT_FIELDS: usize = 8;
pub(crate) const PROFILE_NAME: &str = "guarded_vector3_dot_return";
const PRIMITIVE_PROFILE_NAME: &str = "primitive_missing_vector_dot_return";
const VECTOR_ROUTE: [&str; 12] = [
    "GetNQuickened", "GetNQuickened", "Mul", "GetNQuickened", "GetNQuickened", "Mul",
    "Add", "GetNQuickened", "GetNQuickened", "Mul", "Add", "Return",
];
const PRIMITIVE_ROUTE: [&str; 18] = [
    "local", "guarded_property_number", "local", "guarded_property_number", "number_multiply",
    "local", "guarded_property_number", "local", "guarded_property_number", "number_multiply",
    "number_add", "local", "guarded_property_number", "local", "guarded_property_number",
    "number_multiply", "number_add", "return",
];

#[derive(Clone)]
struct LiteralObject {
    fields: Vec<(crate::value::PropertyName, f64)>,
}

#[derive(Clone)]
enum DotInput {
    Literal(LiteralObject),
    PrimitiveString,
}

#[derive(Clone)]
pub(crate) struct FreshObjectCallSelection {
    callee_slot: u16,
    inputs: [DotInput; 2],
    span: u8,
}

pub(crate) struct NativeFreshObjectCallPlan {
    selection: FreshObjectCallSelection,
    fact: Option<crate::function_call_fact::VectorDotReturn>,
    intrinsic_generation: Option<u64>,
}

impl NativeFreshObjectCallPlan {
    pub(crate) const fn new(selection: FreshObjectCallSelection) -> Self {
        Self {
            selection,
            fact: None,
            intrinsic_generation: None,
        }
    }

    pub(crate) fn execute(&mut self, environment: &crate::environment::Environment) -> Option<f64> {
        let target = environment.retain_proven_function(self.selection.callee_slot)?;
        let fact = crate::function_call_fact::stable_vector_dot_return(&mut self.fact, &target)?;
        if self.selection.has_primitive() {
            self.validate_primitive_fields(&fact.fields)?;
            return Some(f64::NAN);
        }
        let left = ordered_input(&self.selection.inputs[0], &fact.fields)?;
        let right = ordered_input(&self.selection.inputs[1], &fact.fields)?;
        let mut context = crate::native_property::NativeVectorDotContext::new(left, right);
        let status = crate::native_property::execute_vector_dot(&mut context);
        context.result(status)
    }

    pub(crate) const fn span(&self) -> usize {
        self.selection.span as usize
    }

    pub(crate) fn profile_name(&self) -> &'static str {
        self.selection.profile_name()
    }

    pub(crate) fn route(&self) -> impl Iterator<Item = &'static str> {
        self.selection.route().iter().copied()
    }

    fn validate_primitive_fields(&mut self, fields: &[std::rc::Rc<str>; 3]) -> Option<()> {
        let generation = primitive_fields_missing(fields)?;
        let expected = self.intrinsic_generation.get_or_insert(generation);
        (*expected == generation).then_some(())
    }
}

impl FreshObjectCallSelection {
    fn has_primitive(&self) -> bool {
        self.inputs.iter().any(|input| matches!(input, DotInput::PrimitiveString))
    }

    fn profile_name(&self) -> &'static str {
        if self.has_primitive() { PRIMITIVE_PROFILE_NAME } else { PROFILE_NAME }
    }

    fn route(&self) -> &'static [&'static str] {
        if self.has_primitive() { &PRIMITIVE_ROUTE } else { &VECTOR_ROUTE }
    }
}

fn ordered_input(input: &DotInput, fields: &[std::rc::Rc<str>; 3]) -> Option<[f64; 3]> {
    let DotInput::Literal(object) = input else { return None };
    ordered_values(object, fields)
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
    let mut inputs = BTreeMap::new();
    for pc in start + 1..entries.len().min(start + MAX_REGION_LEN) {
        let instruction = entries[pc].instruction;
        if instruction.opcode == Opcode::Call {
            return finish_selection(code, cfg, start, pc, callee, instruction, &inputs);
        }
        collect_definition(code, instruction, pc, &mut values, &mut inputs)?;
    }
    None
}

pub(crate) fn eager_candidate(code: CodeView<'_>) -> bool {
    if code.instruction(0).is_none_or(|op| op.opcode != Opcode::LoadLocal) {
        return false;
    }
    let mut inputs = 0;
    for pc in 1..code.len().min(MAX_REGION_LEN) {
        let Some(instruction) = code.instruction(pc) else { return false };
        match instruction.opcode {
            Opcode::LoadConst if primitive_string(code, instruction) => inputs += 1,
            Opcode::Slow if matches!(code.cold_at(pc), Some(Op::MakeObject { .. })) => inputs += 1,
            Opcode::Call if inputs == 2 => return is_followed_by_return(code, pc),
            _ => {}
        }
    }
    false
}

fn primitive_string(code: CodeView<'_>, instruction: Instruction) -> bool {
    matches!(
        code.constant(instruction.b),
        Some(Constant::String(_) | Constant::StringUnits(_))
    )
}

fn is_followed_by_return(code: CodeView<'_>, pc: usize) -> bool {
    code.instruction(pc + 1)
        .is_some_and(|instruction| instruction.opcode == Opcode::Return)
}

fn collect_definition(
    code: CodeView<'_>,
    instruction: Instruction,
    pc: usize,
    values: &mut BTreeMap<Register, f64>,
    inputs: &mut BTreeMap<Register, DotInput>,
) -> Option<()> {
    match instruction.opcode {
        Opcode::LoadConst => collect_constant(code, instruction, values, inputs),
        Opcode::Unary => collect_unary(instruction, values),
        Opcode::Slow => collect_object(code.cold_at(pc)?, values, inputs),
        _ => None,
    }
}

fn collect_constant(
    code: CodeView<'_>,
    instruction: Instruction,
    values: &mut BTreeMap<Register, f64>,
    inputs: &mut BTreeMap<Register, DotInput>,
) -> Option<()> {
    match code.constant(instruction.b)? {
        Constant::Number(value) => {
            values.insert(instruction.a, *value);
            Some(())
        }
        Constant::String(_) | Constant::StringUnits(_) => {
            inputs.insert(instruction.a, DotInput::PrimitiveString);
            Some(())
        }
        _ => None,
    }
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
    inputs: &mut BTreeMap<Register, DotInput>,
) -> Option<()> {
    let Op::MakeObject { dst, properties } = operation else {
        return None;
    };
    (!properties.is_empty() && properties.len() <= MAX_OBJECT_FIELDS).then_some(())?;
    let fields = properties
        .iter()
        .map(|(name, source)| Some((name.clone(), *values.get(source)?)))
        .collect::<Option<Vec<_>>>()?;
    inputs.insert(*dst, DotInput::Literal(LiteralObject { fields }));
    Some(())
}

fn finish_selection(
    code: CodeView<'_>,
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
    call_pc: usize,
    callee: Instruction,
    call: Instruction,
    inputs: &BTreeMap<Register, DotInput>,
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
        inputs: [inputs.get(left)?.clone(), inputs.get(right)?.clone()],
        span: u8::try_from(end.checked_sub(start)?).ok()?,
    })
}

fn primitive_fields_missing(fields: &[std::rc::Rc<str>; 3]) -> Option<u64> {
    use crate::ops::Builtin::{ObjectPrototype, StringPrototype};
    (crate::builtins::read_intrinsic_prototype_override(StringPrototype).is_none()
        && crate::builtins::read_intrinsic_prototype_override(ObjectPrototype).is_none())
        .then_some(())?;
    for field in fields {
        primitive_field_missing(field, StringPrototype, ObjectPrototype)?;
    }
    Some(crate::builtins::intrinsic_override_generation())
}

fn primitive_field_missing(
    field: &str,
    string: crate::ops::Builtin,
    object: crate::ops::Builtin,
) -> Option<()> {
    (field != "length" && crate::arrays::array_index(field).is_none()).then_some(())?;
    for builtin in [string, object] {
        crate::builtins::read_intrinsic_override(builtin, field)
            .is_none()
            .then_some(())?;
        matches!(crate::builtins::property(builtin, field), crate::value::Value::Undefined)
            .then_some(())?;
    }
    Some(())
}
