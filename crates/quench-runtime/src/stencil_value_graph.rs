//! Bounded disposable value/use graph over canonical residual instructions.
//!
//! This module owns no JavaScript semantics and no durable optimizer state.

use crate::ir::{Instruction, Opcode, Register};
use crate::stencil_plan::{
    fold_numeric_sources, numeric_operation, DiscardedRegisters, F64x3Bindings, FusionCost,
    LocalBinarySelection, LocalNumericInputs, LocalPropertySelection, NumericDefinition,
    NumericProducer, NumericSource, MAX_BLOCK_VALUES,
};
use std::collections::BTreeSet;

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
            store_slot: None,
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
            store_slot: None,
            discarded: self.discarded_registers(operation.a),
            cost,
        })
    }

    #[cfg(test)]
    pub(crate) fn value(&self, id: ValueId) -> Option<ValueNode> {
        self.node(id)
    }

    #[cfg(test)]
    pub(crate) fn current_value(&self, register: Register) -> Option<ValueId> {
        self.current(register)
    }

    pub(crate) fn marked_len(&self, roots: &[Register]) -> usize {
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
