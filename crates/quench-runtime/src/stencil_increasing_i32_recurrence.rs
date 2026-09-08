//! Counted affine-I32 recurrences selected from canonical residual dataflow.

use crate::stencil_value_graph::{ValueDefinition, ValueGraph, ValueId};
use crate::{ir::Opcode, machine::CodeView};

const MAX_BODY_VALUES: usize = 24;

#[derive(Clone, Copy)]
pub(crate) struct IncreasingI32Recurrence {
    counted: crate::stencil_counted_loop::CountedLoop,
    initial: i32,
    multiplier: i32,
    addend: i32,
    result: crate::stencil_counted_function::ReturnedRepresentation,
}

impl IncreasingI32Recurrence {
    pub(crate) fn execute_native(self) -> Option<f64> {
        let (value, native) = crate::function_counter_recurrence::execute_increasing(
            self.initial,
            self.counted.start,
            self.counted.end,
            self.multiplier,
            self.addend,
        )?;
        native.then_some(())?;
        Some(match self.result {
            crate::stencil_counted_function::ReturnedRepresentation::Direct => f64::from(value),
            crate::stencil_counted_function::ReturnedRepresentation::U32 => f64::from(value as u32),
        })
    }
}

pub(crate) fn select_function(code: CodeView<'_>) -> Option<IncreasingI32Recurrence> {
    let facts = crate::stencil_counted_function::select(code, |counted, body, per_iteration| {
        (per_iteration == [counted.index_slot]).then_some(())?;
        let (state_slot, multiplier, addend) = select_body(body, counted.index_slot)?;
        Some((counted, state_slot, multiplier, addend))
    })?;
    let (counted, state_slot, multiplier, addend) = facts.loop_cover;
    (facts.returned.slot == state_slot && state_slot != counted.index_slot).then_some(())?;
    let initial = crate::stencil_counted_function::initial_f64(&facts.initials, state_slot)?;
    Some(IncreasingI32Recurrence {
        counted,
        initial: crate::stencil_numeric_integer_selection::exact_i32(initial)?,
        multiplier,
        addend,
        result: facts.returned.representation,
    })
}

fn select_body(code: CodeView<'_>, index_slot: u16) -> Option<(u16, i32, i32)> {
    let mut graph = ValueGraph::<MAX_BODY_VALUES>::new();
    let mut stored = None;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        match instruction.opcode {
            Opcode::LoadLocalChecked => graph
                .push_guarded_local(instruction.a, instruction.b)
                .then_some(())?,
            Opcode::LoadLocal
            | Opcode::LoadConst
            | Opcode::Move
            | Opcode::Mul
            | Opcode::Add
            | Opcode::AddConst => graph
                .push(instruction, |constant| number_bits(code, constant))
                .then_some(())?,
            Opcode::Binary => graph.push_i32_binary(instruction).then_some(())?,
            Opcode::StoreLocal => {
                stored = Some((instruction.a, graph.current(instruction.b)?));
            }
            _ => return None,
        }
    }
    select_expression(&graph, stored?, index_slot)
}

fn select_expression(
    graph: &ValueGraph<MAX_BODY_VALUES>,
    stored: (u16, ValueId),
    index_slot: u16,
) -> Option<(u16, i32, i32)> {
    let value = strip_i32_coercion(graph, stored.1)?;
    let ValueDefinition::AddConstant { source, bits, left } = graph.node(value)?.definition else {
        return None;
    };
    (!left).then_some(())?;
    let addend = crate::vm::numeric_to_int32(f64::from_bits(bits));
    let (state_slot, multiplier) = select_sum(graph, source, index_slot)?;
    (stored.0 == state_slot).then_some((state_slot, multiplier, addend))
}

fn strip_i32_coercion(graph: &ValueGraph<MAX_BODY_VALUES>, id: ValueId) -> Option<ValueId> {
    let id = graph.canonical(id)?;
    let ValueDefinition::Binary { operator, lhs, rhs } = graph.node(id)?.definition else {
        return None;
    };
    (operator == crate::ops::BinaryOp::BitwiseOr).then_some(())?;
    zero_side(graph, lhs, rhs)
}

fn select_sum(
    graph: &ValueGraph<MAX_BODY_VALUES>,
    id: ValueId,
    index_slot: u16,
) -> Option<(u16, i32)> {
    let ValueDefinition::Binary { operator, lhs, rhs } =
        graph.node(graph.canonical(id)?)?.definition
    else {
        return None;
    };
    (operator == crate::ops::BinaryOp::Add).then_some(())?;
    select_product(graph, lhs, rhs, index_slot)
        .or_else(|| select_product(graph, rhs, lhs, index_slot))
}

fn select_product(
    graph: &ValueGraph<MAX_BODY_VALUES>,
    product: ValueId,
    index: ValueId,
    index_slot: u16,
) -> Option<(u16, i32)> {
    (local_source(graph, index)? == index_slot).then_some(())?;
    let ValueDefinition::Binary { operator, lhs, rhs } =
        graph.node(graph.canonical(product)?)?.definition
    else {
        return None;
    };
    (operator == crate::ops::BinaryOp::Multiply).then_some(())?;
    constant_side(graph, lhs, rhs)
}

fn constant_side(
    graph: &ValueGraph<MAX_BODY_VALUES>,
    lhs: ValueId,
    rhs: ValueId,
) -> Option<(u16, i32)> {
    local_source(graph, lhs)
        .zip(constant_i32(graph, rhs))
        .or_else(|| local_source(graph, rhs).zip(constant_i32(graph, lhs)))
}

fn zero_side(graph: &ValueGraph<MAX_BODY_VALUES>, lhs: ValueId, rhs: ValueId) -> Option<ValueId> {
    constant_i32(graph, rhs)
        .filter(|value| *value == 0)
        .and_then(|_| graph.canonical(lhs))
        .or_else(|| {
            constant_i32(graph, lhs)
                .filter(|value| *value == 0)
                .and_then(|_| graph.canonical(rhs))
        })
}

fn local_source(graph: &ValueGraph<MAX_BODY_VALUES>, id: ValueId) -> Option<u16> {
    match graph.node(graph.canonical(id)?)?.definition {
        ValueDefinition::Source(crate::stencil_plan::NumericSource::Local(slot)) => Some(slot),
        _ => None,
    }
}

fn constant_i32(graph: &ValueGraph<MAX_BODY_VALUES>, id: ValueId) -> Option<i32> {
    let crate::stencil_plan::NumericSource::Constant(bits) = graph.resolve(id)? else {
        return None;
    };
    Some(crate::vm::numeric_to_int32(f64::from_bits(bits)))
}

fn number_bits(code: CodeView<'_>, constant: u16) -> Option<u64> {
    let crate::ops::Constant::Number(value) = code.constant(constant)? else {
        return None;
    };
    Some(value.to_bits())
}
