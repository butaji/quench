//! Bounded, disposable selection over canonical residual instructions.
//!
//! This module owns physical bindings and cost decisions, never JavaScript
//! semantics. Selected plans refer back to immutable residual operations.

use crate::ir::{Instruction, Opcode, Register};
use std::collections::BTreeSet;

pub(crate) const MAX_BLOCK_VALUES: usize = 8;
pub(crate) type DiscardedRegisters = [Option<Register>; MAX_BLOCK_VALUES];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct F64x3Bindings {
    pub inputs: [Register; 3],
    pub output: Register,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FusionCost {
    removed_dispatches: u8,
    removed_materializations: u8,
    added_transfers: u8,
}

impl FusionCost {
    const ADD_CHAIN: Self = Self {
        removed_dispatches: 1,
        removed_materializations: 1,
        added_transfers: 0,
    };

    const LOCAL_CONSTANT: Self = Self {
        removed_dispatches: 1,
        removed_materializations: 1,
        added_transfers: 1,
    };

    fn numeric_producers(count: usize) -> Self {
        let count = u8::try_from(count).unwrap_or(u8::MAX);
        Self {
            removed_dispatches: count,
            removed_materializations: count,
            added_transfers: 2,
        }
    }

    fn constant_fold(count: usize) -> Self {
        let count = u8::try_from(count).unwrap_or(u8::MAX);
        Self {
            removed_dispatches: count.saturating_add(1),
            removed_materializations: count.saturating_add(1),
            added_transfers: 1,
        }
    }

    fn property_producers(count: usize) -> Self {
        let count = u8::try_from(count).unwrap_or(u8::MAX);
        Self {
            removed_dispatches: count,
            removed_materializations: count,
            added_transfers: 1,
        }
    }

    const fn profitable(self) -> bool {
        self.removed_dispatches + self.removed_materializations > self.added_transfers
    }

    const fn rank(self) -> u8 {
        self.removed_dispatches
            .saturating_add(self.removed_materializations)
            .saturating_sub(self.added_transfers)
    }
}

pub(crate) trait RankedSelection {
    fn rank(&self) -> (u8, u8);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AddChainSelection {
    pub bindings: F64x3Bindings,
    pub cost: FusionCost,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NumericSource {
    Local(u16),
    Constant(u64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NumericProducer {
    pub output: Register,
    pub definition: NumericDefinition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NumericDefinition {
    Source(NumericSource),
    Alias(Register),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ValueId {
    pub register: Register,
    pub version: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ValueDefinition {
    Source(NumericSource),
    Alias(ValueId),
    AddConstant {
        source: ValueId,
        bits: u64,
        left: bool,
    },
    Binary {
        operator: crate::ops::BinaryOp,
        lhs: ValueId,
        rhs: ValueId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ValueNode {
    pub id: ValueId,
    pub definition: ValueDefinition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LocalNumericInputs {
    Sources([NumericSource; 2]),
    SlotConstant { slot: u16, bits: u64 },
    AddChain {
        sources: [NumericSource; 3],
        bindings: F64x3Bindings,
    },
    Folded { bits: u64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LocalBinarySelection {
    pub inputs: LocalNumericInputs,
    pub output: Register,
    pub operation: Instruction,
    pub span: u8,
    pub discarded: DiscardedRegisters,
    pub cost: FusionCost,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LocalPropertySelection {
    pub receiver_slot: u16,
    pub output: Register,
    pub operation: Instruction,
    pub span: u8,
    pub discarded: DiscardedRegisters,
    pub cost: FusionCost,
}

impl RankedSelection for LocalBinarySelection {
    fn rank(&self) -> (u8, u8) {
        (self.cost.rank(), self.span)
    }
}

impl RankedSelection for LocalPropertySelection {
    fn rank(&self) -> (u8, u8) {
        (self.cost.rank(), self.span)
    }
}

/// Disposable value/use view for one bounded straight-line residual window.
///
/// Instructions remain the semantic authority. Nodes are bounded value facts,
/// not executable operations or a second semantic IR.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BlockValueGraph {
    nodes: [ValueNode; MAX_BLOCK_VALUES],
    len: u8,
}

impl BlockValueGraph {
    pub(crate) const fn new() -> Self {
        const EMPTY: ValueNode = ValueNode {
            id: ValueId {
                register: 0,
                version: 0,
            },
            definition: ValueDefinition::Alias(ValueId {
                register: 0,
                version: 0,
            }),
        };
        Self {
            nodes: [EMPTY; MAX_BLOCK_VALUES],
            len: 0,
        }
    }

    pub(crate) fn push(
        &mut self,
        instruction: Instruction,
        constant_bits: impl FnOnce(u16) -> Option<u64>,
    ) -> bool {
        if usize::from(self.len) == MAX_BLOCK_VALUES {
            return false;
        }
        let Some(mut node) = self.value_node(instruction, constant_bits) else {
            return false;
        };
        if let Some(existing) = self
            .nodes()
            .iter()
            .find(|existing| existing.definition == node.definition)
        {
            node.definition = ValueDefinition::Alias(existing.id);
        }
        self.nodes[usize::from(self.len)] = node;
        self.len += 1;
        true
    }

    pub(crate) fn select(
        &self,
        operation: Instruction,
        live_after: &BTreeSet<Register>,
    ) -> Option<LocalBinarySelection> {
        let operator = numeric_operation(operation)?;
        let direct = self
            .resolve_register(operation.b)
            .zip(self.resolve_register(operation.c));
        if let Some((lhs, rhs)) = direct {
            return self.select_resolved(operation, operator, [lhs, rhs], live_after);
        }
        self.select_add_tree(operation, operator, live_after)
    }

    pub(crate) fn select_property(
        &self,
        operation: Instruction,
        live_after: &BTreeSet<Register>,
    ) -> Option<LocalPropertySelection> {
        select_local_property(self, operation, live_after)
    }

    pub(crate) fn select_add_const(
        &self,
        operation: Instruction,
        bits: u64,
        live_after: &BTreeSet<Register>,
    ) -> Option<LocalBinarySelection> {
        if operation.opcode != Opcode::AddConst {
            return None;
        }
        let source = self.resolve_register(operation.b)?;
        let inputs = if operation.add_const_is_left() {
            [NumericSource::Constant(bits), source]
        } else {
            [source, NumericSource::Constant(bits)]
        };
        self.select_resolved(operation, crate::ops::BinaryOp::Add, inputs, live_after)
    }

    pub(crate) fn first(&self) -> Option<NumericProducer> {
        let node = self.nodes().first()?;
        let definition = match node.definition {
            ValueDefinition::Source(source) => NumericDefinition::Source(source),
            ValueDefinition::Alias(id) => NumericDefinition::Alias(id.register),
            ValueDefinition::AddConstant { .. } | ValueDefinition::Binary { .. } => return None,
        };
        Some(NumericProducer {
            output: node.id.register,
            definition,
        })
    }

    pub(crate) const fn len(self) -> usize {
        self.len as usize
    }

    fn nodes(&self) -> &[ValueNode] {
        &self.nodes[..usize::from(self.len)]
    }

    fn value_node(
        &self,
        instruction: Instruction,
        constant_bits: impl FnOnce(u16) -> Option<u64>,
    ) -> Option<ValueNode> {
        let flow = instruction.register_flow();
        if !flow.complete {
            return None;
        }
        let id = self.next_id(flow.definition?)?;
        let definition = self.value_definition(instruction, constant_bits)?;
        Some(ValueNode { id, definition })
    }

    fn value_definition(
        &self,
        instruction: Instruction,
        constant_bits: impl FnOnce(u16) -> Option<u64>,
    ) -> Option<ValueDefinition> {
        let definition = match instruction.opcode {
            Opcode::LoadLocal if pure(instruction.opcode) => {
                ValueDefinition::Source(NumericSource::Local(instruction.b))
            }
            Opcode::LoadConst if pure(instruction.opcode) => {
                ValueDefinition::Source(NumericSource::Constant(constant_bits(instruction.b)?))
            }
            Opcode::Move if instruction.flags == 0 && pure(instruction.opcode) => {
                ValueDefinition::Alias(self.canonical(self.current(instruction.b)?)?)
            }
            Opcode::AddConst => self.add_constant_definition(instruction, constant_bits)?,
            opcode if opcode.has_guard(crate::facts::OperationGuard::Number) => {
                self.binary_definition(instruction)?
            }
            _ => return None,
        };
        Some(definition)
    }

    fn binary_definition(&self, instruction: Instruction) -> Option<ValueDefinition> {
        let lhs = self.canonical(self.current(instruction.b)?)?;
        let rhs = self.canonical(self.current(instruction.c)?)?;
        Some(ValueDefinition::Binary {
            operator: numeric_operation(instruction)?,
            lhs,
            rhs,
        })
    }

    fn add_constant_definition(
        &self,
        instruction: Instruction,
        constant_bits: impl FnOnce(u16) -> Option<u64>,
    ) -> Option<ValueDefinition> {
        let source = self.canonical(self.current(instruction.b)?)?;
        matches!(self.resolve(source)?, NumericSource::Constant(_)).then_some(())?;
        Some(ValueDefinition::AddConstant {
            source,
            bits: constant_bits(instruction.c)?,
            left: instruction.add_const_is_left(),
        })
    }

    fn next_id(&self, register: Register) -> Option<ValueId> {
        let version = self
            .nodes()
            .iter()
            .filter(|node| node.id.register == register)
            .count();
        Some(ValueId {
            register,
            version: u8::try_from(version).ok()?,
        })
    }

    fn current(&self, register: Register) -> Option<ValueId> {
        self.nodes()
            .iter()
            .rfind(|node| node.id.register == register)
            .map(|node| node.id)
    }

    fn node(&self, id: ValueId) -> Option<ValueNode> {
        self.nodes().iter().copied().find(|node| node.id == id)
    }

    fn canonical(&self, mut id: ValueId) -> Option<ValueId> {
        for _ in 0..self.len() {
            match self.node(id)?.definition {
                ValueDefinition::Alias(next) => id = next,
                _ => return Some(id),
            }
        }
        None
    }

    fn resolve_register(&self, register: Register) -> Option<NumericSource> {
        self.resolve(self.current(register)?)
    }

    fn resolve(&self, id: ValueId) -> Option<NumericSource> {
        match self.node(id)?.definition {
            ValueDefinition::Source(source) => Some(source),
            ValueDefinition::Alias(source) => self.resolve(source),
            ValueDefinition::AddConstant { source, bits, left } => {
                let source = self.resolve(source)?;
                let inputs = if left {
                    [NumericSource::Constant(bits), source]
                } else {
                    [source, NumericSource::Constant(bits)]
                };
                fold_numeric_sources(inputs, crate::ops::BinaryOp::Add)
                    .map(NumericSource::Constant)
            }
            ValueDefinition::Binary { operator, lhs, rhs } => {
                let inputs = [self.resolve(lhs)?, self.resolve(rhs)?];
                fold_numeric_sources(inputs, operator).map(NumericSource::Constant)
            }
        }
    }

    fn has_unsupported_live_out(&self, output: Register, live: &BTreeSet<Register>) -> bool {
        self.nodes()
            .iter()
            .any(|node| node.id.register != output && live.contains(&node.id.register))
    }

    fn discarded_registers(&self, output: Register) -> DiscardedRegisters {
        let mut discarded = [None; MAX_BLOCK_VALUES];
        let mut length = 0;
        for register in self.nodes().iter().map(|node| node.id.register) {
            if register != output && !discarded.contains(&Some(register)) {
                discarded[length] = Some(register);
                length += 1;
            }
        }
        discarded
    }

    fn select_resolved(
        &self,
        operation: Instruction,
        operator: crate::ops::BinaryOp,
        inputs: [NumericSource; 2],
        live_after: &BTreeSet<Register>,
    ) -> Option<LocalBinarySelection> {
        if self.has_unsupported_live_out(operation.a, live_after) {
            return None;
        }
        let folded = fold_numeric_sources(inputs, operator);
        let marked = self.marked_len(&[operation.b, operation.c]);
        let cost = folded.map_or_else(
            || FusionCost::numeric_producers(marked),
            |_| FusionCost::constant_fold(marked),
        );
        cost.profitable().then_some(LocalBinarySelection {
            inputs: folded.map_or(LocalNumericInputs::Sources(inputs), |bits| {
                LocalNumericInputs::Folded { bits }
            }),
            output: operation.a,
            operation,
            span: u8::try_from(self.len() + 1).ok()?,
            discarded: self.discarded_registers(operation.a),
            cost,
        })
    }

    fn select_add_tree(
        &self,
        operation: Instruction,
        operator: crate::ops::BinaryOp,
        live_after: &BTreeSet<Register>,
    ) -> Option<LocalBinarySelection> {
        if operator != crate::ops::BinaryOp::Add {
            return None;
        }
        let inner = self.canonical(self.current(operation.b)?)?;
        let ValueDefinition::Binary { operator, lhs, rhs } = self.node(inner)?.definition else {
            return None;
        };
        if operator != crate::ops::BinaryOp::Add {
            return None;
        }
        let sources = [self.resolve(lhs)?, self.resolve(rhs)?, self.resolve_register(operation.c)?];
        let bindings = F64x3Bindings {
            inputs: [lhs.register, rhs.register, operation.c],
            output: operation.a,
        };
        self.select_add_tree_sources(operation, sources, bindings, live_after)
    }

    fn select_add_tree_sources(
        &self,
        operation: Instruction,
        sources: [NumericSource; 3],
        bindings: F64x3Bindings,
        live_after: &BTreeSet<Register>,
    ) -> Option<LocalBinarySelection> {
        if self.has_unsupported_live_out(operation.a, live_after) {
            return None;
        }
        let cost = FusionCost::numeric_producers(self.marked_len(&[operation.b, operation.c]));
        cost.profitable().then_some(LocalBinarySelection {
            inputs: LocalNumericInputs::AddChain { sources, bindings },
            output: operation.a,
            operation,
            span: u8::try_from(self.len() + 1).ok()?,
            discarded: self.discarded_registers(operation.a),
            cost,
        })
    }

    #[cfg(test)]
    fn value(&self, id: ValueId) -> Option<ValueNode> {
        self.node(id)
    }

    #[cfg(test)]
    fn current_value(&self, register: Register) -> Option<ValueId> {
        self.current(register)
    }

    fn marked_len(&self, roots: &[Register]) -> usize {
        let mut marked = [false; MAX_BLOCK_VALUES];
        for root in roots.iter().filter_map(|register| self.current(*register)) {
            self.mark(root, &mut marked);
        }
        marked.into_iter().filter(|marked| *marked).count()
    }

    fn mark(&self, id: ValueId, marked: &mut [bool; MAX_BLOCK_VALUES]) {
        let Some(index) = self.nodes().iter().position(|node| node.id == id) else {
            return;
        };
        if std::mem::replace(&mut marked[index], true) {
            return;
        }
        match self.nodes[index].definition {
            ValueDefinition::Alias(source) => self.mark(source, marked),
            ValueDefinition::AddConstant { source, .. } => self.mark(source, marked),
            ValueDefinition::Binary { lhs, rhs, .. } => {
                self.mark(lhs, marked);
                self.mark(rhs, marked);
            }
            ValueDefinition::Source(_) => {}
        }
    }
}

fn pure(opcode: Opcode) -> bool {
    opcode.effects() == &[crate::facts::OperationEffect::Pure]
}

fn select_local_property(
    graph: &BlockValueGraph,
    operation: Instruction,
    live_after: &BTreeSet<Register>,
) -> Option<LocalPropertySelection> {
    if graph.len() == 0 || operation.opcode != Opcode::GetN || operation.flags != 0 {
        return None;
    }
    let NumericSource::Local(receiver_slot) = graph.resolve_register(operation.b)? else {
        return None;
    };
    let lost_live = graph.has_unsupported_live_out(operation.a, live_after);
    let cost = FusionCost::property_producers(graph.marked_len(&[operation.b]));
    if lost_live || !cost.profitable() {
        return None;
    }
    Some(LocalPropertySelection {
        receiver_slot,
        output: operation.a,
        operation,
        span: u8::try_from(graph.len() + 1).ok()?,
        discarded: graph.discarded_registers(operation.a),
        cost,
    })
}

pub(crate) fn select_add_chain(
    first: Instruction,
    second: Instruction,
    live_after: &BTreeSet<Register>,
) -> Option<AddChainSelection> {
    let bindings = add_chain_bindings(first, second)?;
    if live_after.contains(&first.a) || !FusionCost::ADD_CHAIN.profitable() {
        return None;
    }
    Some(AddChainSelection {
        bindings,
        cost: FusionCost::ADD_CHAIN,
    })
}

pub(crate) fn select_local_binary(
    producers: &[NumericProducer],
    operation: Instruction,
    live_after: &BTreeSet<Register>,
) -> Option<LocalBinarySelection> {
    if !(2..=MAX_BLOCK_VALUES).contains(&producers.len()) || duplicate_definitions(producers) {
        return None;
    }
    let operator = numeric_operation(operation)?;
    let inputs = operation_sources(producers, operation)?;
    let overwritten = operation.a;
    let lost_live_value = producers
        .iter()
        .any(|producer| producer.output != overwritten && live_after.contains(&producer.output));
    let folded = fold_numeric_sources(inputs, operator);
    let cost = folded.map_or_else(
        || FusionCost::numeric_producers(producers.len()),
        |_| FusionCost::constant_fold(producers.len()),
    );
    if lost_live_value || !cost.profitable() {
        return None;
    }
    Some(LocalBinarySelection {
        inputs: folded.map_or(LocalNumericInputs::Sources(inputs), |bits| {
            LocalNumericInputs::Folded { bits }
        }),
        output: operation.a,
        operation,
        span: u8::try_from(producers.len() + 1).ok()?,
        discarded: discarded_registers(producers, operation.a),
        cost,
    })
}

fn fold_numeric_sources(inputs: [NumericSource; 2], operator: crate::ops::BinaryOp) -> Option<u64> {
    let [NumericSource::Constant(lhs), NumericSource::Constant(rhs)] = inputs else {
        return None;
    };
    let lhs = f64::from_bits(lhs);
    let rhs = f64::from_bits(rhs);
    use crate::ops::BinaryOp::{Add, Divide, Multiply, Subtract};
    let value = match operator {
        Add => lhs + rhs,
        Subtract => lhs - rhs,
        Multiply => lhs * rhs,
        Divide => lhs / rhs,
        _ => return None,
    };
    Some(value.to_bits())
}

fn numeric_operation(instruction: Instruction) -> Option<crate::ops::BinaryOp> {
    use crate::ops::BinaryOp::{Add, Divide, Multiply, Subtract};
    let operator = match instruction.opcode {
        Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div if instruction.flags == 0 => {
            instruction.opcode.numeric_operator()?
        }
        Opcode::Binary => crate::ir::compact_binary_operator(instruction.flags)?,
        _ => return None,
    };
    matches!(operator, Add | Subtract | Multiply | Divide).then_some(operator)
}

pub(crate) fn select_source_add_const(
    producer: NumericProducer,
    operation: Instruction,
    constant_bits: u64,
    live_after: &BTreeSet<Register>,
) -> Option<LocalBinarySelection> {
    if operation.opcode != Opcode::AddConst
        || operation.b != producer.output
        || (producer.output != operation.a && live_after.contains(&producer.output))
        || !FusionCost::LOCAL_CONSTANT.profitable()
    {
        return None;
    }
    let inputs = add_const_inputs(producer.definition, operation, constant_bits)?;
    Some(LocalBinarySelection {
        inputs,
        output: operation.a,
        operation,
        span: 2,
        discarded: discarded_registers(&[producer], operation.a),
        cost: FusionCost::LOCAL_CONSTANT,
    })
}

fn add_const_inputs(
    definition: NumericDefinition,
    operation: Instruction,
    constant_bits: u64,
) -> Option<LocalNumericInputs> {
    match definition {
        NumericDefinition::Source(NumericSource::Local(slot)) if operation.flags == 0 => {
            Some(LocalNumericInputs::SlotConstant {
                slot,
                bits: constant_bits,
            })
        }
        NumericDefinition::Source(NumericSource::Constant(source_bits)) => {
            let (lhs, rhs) = if operation.add_const_is_left() {
                (constant_bits, source_bits)
            } else {
                (source_bits, constant_bits)
            };
            let bits = fold_numeric_sources(
                [NumericSource::Constant(lhs), NumericSource::Constant(rhs)],
                crate::ops::BinaryOp::Add,
            )?;
            Some(LocalNumericInputs::Folded { bits })
        }
        _ => None,
    }
}

fn discarded_registers(producers: &[NumericProducer], output: Register) -> DiscardedRegisters {
    let mut discarded = [None; MAX_BLOCK_VALUES];
    let mut length = 0;
    for producer in producers.iter().map(|producer| producer.output) {
        if producer != output && !discarded.contains(&Some(producer)) {
            discarded[length] = Some(producer);
            length += 1;
        }
    }
    discarded
}

fn operation_sources(
    producers: &[NumericProducer],
    operation: Instruction,
) -> Option<[NumericSource; 2]> {
    Some([
        resolve_source(producers, operation.b)?,
        resolve_source(producers, operation.c)?,
    ])
}

fn resolve_source(producers: &[NumericProducer], mut register: Register) -> Option<NumericSource> {
    let mut end = producers.len();
    for _ in 0..producers.len() {
        let index = producers[..end]
            .iter()
            .rposition(|producer| producer.output == register)?;
        match producers[index].definition {
            NumericDefinition::Source(source) => return Some(source),
            NumericDefinition::Alias(input) => {
                register = input;
                end = index;
            }
        }
    }
    None
}

fn duplicate_definitions(producers: &[NumericProducer]) -> bool {
    producers.iter().enumerate().any(|(index, producer)| {
        producers[..index]
            .iter()
            .any(|prior| prior.output == producer.output)
    })
}

fn add_chain_bindings(first: Instruction, second: Instruction) -> Option<F64x3Bindings> {
    let is_numeric_add = |instruction: Instruction| {
        instruction.opcode == Opcode::Add
            && instruction.flags == 0
            && instruction
                .opcode
                .has_guard(crate::facts::OperationGuard::Number)
    };
    if !is_numeric_add(first) || !is_numeric_add(second) {
        return None;
    }
    if second.b != first.a || second.c == first.a {
        return None;
    }
    Some(F64x3Bindings {
        inputs: [first.b, first.c, second.c],
        output: second.a,
    })
}

#[cfg(test)]
#[path = "stencil_plan_tests.rs"]
mod tests;
