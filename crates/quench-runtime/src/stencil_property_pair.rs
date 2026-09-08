//! Two-shape property/call composition over canonical residual and IC facts.

use crate::machine::{BaselineEntry, CodeView};

const REGION_LEN: usize = 8;
pub(crate) const PROFILE_NAME: &str = "polymorphic_property_pair_sum_return";

#[derive(Clone, Copy)]
pub(crate) struct PropertyPairSelection {
    callee_slot: u16,
    receiver_slots: [u16; 2],
}

pub(crate) struct NativePropertyPairPlan {
    selection: PropertyPairSelection,
    fact: Option<crate::function_call_fact::OwnFieldAddReturn>,
}

impl NativePropertyPairPlan {
    pub(crate) const fn new(selection: PropertyPairSelection) -> Self {
        Self {
            selection,
            fact: None,
        }
    }

    pub(crate) fn execute(&mut self, environment: &crate::environment::Environment) -> Option<i32> {
        let target = environment.retain_proven_function(self.selection.callee_slot)?;
        let fact = self.target_fact(&target)?;
        environment.with_proven_object(self.selection.receiver_slots[0], |left| {
            environment.with_proven_object(self.selection.receiver_slots[1], |right| {
                execute_pair(&target, &fact, left, right)
            })?
        })?
    }

    fn target_fact(
        &mut self,
        target: &crate::value::FunctionValue,
    ) -> Option<crate::function_call_fact::OwnFieldAddReturn> {
        crate::function_call_fact::stable_own_field_add_return(&mut self.fact, target)
    }

    pub(crate) const fn span(&self) -> usize {
        REGION_LEN
    }
}

fn execute_pair(
    target: &crate::value::FunctionValue,
    fact: &crate::function_call_fact::OwnFieldAddReturn,
    left: &crate::value::ObjectData,
    right: &crate::value::ObjectData,
) -> Option<i32> {
    let code = target.code.code()?;
    let access = [
        guarded_slot(code, left, &fact.field)?,
        guarded_slot(code, right, &fact.field)?,
    ];
    let mut context = crate::native_property::NativePropertyPairContext::new(access, fact.addend);
    let status = crate::native_property::execute_property_pair_i32(&mut context);
    context.result(status)
}

fn guarded_slot(
    code: CodeView<'_>,
    object: &crate::value::ObjectData,
    key: &str,
) -> Option<crate::native_property::GuardedPropertySlot> {
    let metadata = code.metadata_at(1)?;
    (metadata.name.as_deref()? == key).then_some(())?;
    let shape = crate::identity::ShapeId(object.semantic_layout_id());
    let property = crate::identity::property_key_id(key);
    let slot = code
        .quickening_site(1)?
        .borrow_mut()
        .probe_shape(shape, property)?;
    object.guarded_plain_slot(shape.0, slot, key)
}

pub(crate) fn select_property_pair(
    _code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<PropertyPairSelection> {
    let ops = entries.get(start..start.checked_add(REGION_LEN)?)?;
    let i = |pc: usize| ops[pc].instruction;
    validate_shape(ops, i)?;
    cfg.region_control(start, start + REGION_LEN)?;
    Some(PropertyPairSelection {
        callee_slot: i(0).b,
        receiver_slots: [i(1).b, i(4).b],
    })
}

fn validate_shape(
    ops: &[BaselineEntry],
    i: impl Fn(usize) -> crate::ir::Instruction,
) -> Option<()> {
    use crate::ir::Opcode::{Add, Call, LoadLocal, Return};
    let expected = [
        LoadLocal, LoadLocal, Call, LoadLocal, LoadLocal, Call, Add, Return,
    ];
    ops.iter()
        .zip(expected)
        .all(|(entry, op)| entry.instruction.opcode == op)
        .then_some(())?;
    (i(0).b == i(3).b
        && i(2).flags == 1
        && i(2).b == i(0).a
        && i(2).c == i(1).a
        && i(5).flags == 1
        && i(5).b == i(3).a
        && i(5).c == i(4).a
        && i(6).b == i(2).a
        && i(6).c == i(5).a
        && i(7) == crate::ir::Instruction::ret(i(6).a))
    .then_some(())
}

pub(crate) fn route() -> impl Iterator<Item = &'static str> {
    [
        "ShapeGuard",
        "FieldLoad",
        "AddConst",
        "ShapeGuard",
        "FieldLoad",
        "AddConst",
        "Add",
        "Return",
    ]
    .into_iter()
}
