//! Costed terminal covers over the shared bounded value graph.

use crate::ir::{Instruction, Opcode, Register};
use crate::stencil_plan::{
    fold_numeric_sources, numeric_operation, DiscardedRegisters, F64x3Bindings, FusionCost,
    LocalBinarySelection, LocalNumericInputs, LocalPropertySelection, NumericDefinition,
    NumericProducer, NumericSeries, NumericSource, MAX_BLOCK_VALUES,
};
use crate::stencil_value_graph::{BlockValueGraph, ValueDefinition, ValueGraph, ValueId};
use std::collections::BTreeSet;

impl ValueGraph<MAX_BLOCK_VALUES> {
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
        self.select_binary_series(operation, operator, live_after)
            .or_else(|| self.select_add_tree(operation, operator, live_after))
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
        (operation.opcode == Opcode::AddConst).then_some(())?;
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
            ValueDefinition::NegateConstant { .. }
            | ValueDefinition::AddConstant { .. }
            | ValueDefinition::Binary { .. }
            | ValueDefinition::IntrinsicBinary { .. } => return None,
        };
        Some(NumericProducer {
            output: node.id.register,
            definition,
        })
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
        (!self.has_unsupported_live_out(operation.a, live_after)).then_some(())?;
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
            result: crate::stencil_plan::LocalResultBinding::register(operation.a),
            operation,
            span: u8::try_from(self.len() + 1).ok()?,
            returns: false,
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
        (operator == crate::ops::BinaryOp::Add).then_some(())?;
        let inner = self.canonical(self.current(operation.b)?)?;
        let ValueDefinition::Binary { operator, lhs, rhs } = self.node(inner)?.definition else {
            return None;
        };
        (operator == crate::ops::BinaryOp::Add).then_some(())?;
        let sources = [
            self.resolve(lhs)?,
            self.resolve(rhs)?,
            self.resolve_register(operation.c)?,
        ];
        let bindings = F64x3Bindings {
            inputs: [lhs.register, rhs.register, operation.c],
            output: operation.a,
        };
        self.select_add_tree_sources(operation, sources, bindings, live_after)
    }

    fn select_binary_series(
        &self,
        operation: Instruction,
        operator: crate::ops::BinaryOp,
        live_after: &BTreeSet<Register>,
    ) -> Option<LocalBinarySelection> {
        series_operation(operator)?;
        let repeated = self.resolve_register(operation.c)?;
        let (base, series) = self.binary_series_base(operation.b, repeated, operator)?;
        (!self.has_unsupported_live_out(operation.a, live_after)).then_some(())?;
        let cost = FusionCost::numeric_producers(self.marked_len(&[operation.b, operation.c]));
        cost.profitable().then_some(LocalBinarySelection {
            inputs: LocalNumericInputs::BinarySeries {
                sources: [base, repeated],
                series,
            },
            result: crate::stencil_plan::LocalResultBinding::register(operation.a),
            operation,
            span: u8::try_from(self.len() + 1).ok()?,
            returns: false,
            discarded: self.discarded_registers(operation.a),
            cost,
        })
    }

    fn binary_series_base(
        &self,
        register: Register,
        repeated: NumericSource,
        final_operator: crate::ops::BinaryOp,
    ) -> Option<(NumericSource, NumericSeries)> {
        let mut value = self.canonical(self.current(register)?)?;
        let mut reversed = [crate::ops::BinaryOp::Add; MAX_BLOCK_VALUES];
        reversed[0] = final_operator;
        let mut len = 1usize;
        loop {
            let ValueDefinition::Binary { operator, lhs, rhs } = self.node(value)?.definition
            else {
                return Some((self.resolve(value)?, NumericSeries::from_reverse(&reversed[..len])?));
            };
            if series_operation(operator).is_none()
                || self.resolve(rhs)? != repeated
                || len == MAX_BLOCK_VALUES
            {
                return None;
            }
            reversed[len] = operator;
            len += 1;
            value = self.canonical(lhs)?;
        }
    }

    fn select_add_tree_sources(
        &self,
        operation: Instruction,
        sources: [NumericSource; 3],
        bindings: F64x3Bindings,
        live_after: &BTreeSet<Register>,
    ) -> Option<LocalBinarySelection> {
        (!self.has_unsupported_live_out(operation.a, live_after)).then_some(())?;
        let cost = FusionCost::numeric_producers(self.marked_len(&[operation.b, operation.c]));
        cost.profitable().then_some(LocalBinarySelection {
            inputs: LocalNumericInputs::AddChain { sources, bindings },
            result: crate::stencil_plan::LocalResultBinding::register(operation.a),
            operation,
            span: u8::try_from(self.len() + 1).ok()?,
            returns: false,
            discarded: self.discarded_registers(operation.a),
            cost,
        })
    }

    #[cfg(test)]
    pub(crate) fn value(&self, id: ValueId) -> Option<crate::stencil_value_graph::ValueNode> {
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
        match self.nodes()[index].definition {
            ValueDefinition::Alias(source) | ValueDefinition::NegateConstant { source } => {
                self.mark(source, marked)
            }
            ValueDefinition::AddConstant { source, .. } => self.mark(source, marked),
            ValueDefinition::Binary { lhs, rhs, .. } => {
                self.mark(lhs, marked);
                self.mark(rhs, marked);
            }
            ValueDefinition::IntrinsicBinary { lhs, rhs, .. } => {
                self.mark(lhs, marked);
                self.mark(rhs, marked);
            }
            ValueDefinition::Source(_) => {}
        }
    }
}

fn series_operation(operator: crate::ops::BinaryOp) -> Option<()> {
    matches!(
        operator,
        crate::ops::BinaryOp::Add
            | crate::ops::BinaryOp::Subtract
            | crate::ops::BinaryOp::Multiply
            | crate::ops::BinaryOp::Divide
    )
    .then_some(())
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
    let required = graph.marked_len(&[operation.b]);
    let invalid = required != graph.len()
        || graph.has_unsupported_live_out(operation.a, live_after)
        || !FusionCost::property_producers(required).profitable();
    (!invalid).then_some(LocalPropertySelection {
        receiver_slot,
        result: crate::stencil_plan::LocalResultBinding::register(operation.a),
        operation,
        span: u8::try_from(graph.len() + 1).ok()?,
        returns: false,
        discarded: graph.discarded_registers(operation.a),
        cost: FusionCost::property_producers(required),
    })
}
