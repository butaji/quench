//! Bounded disposable value/use graph; it owns neither JS semantics nor durable state.

use crate::ir::{Instruction, Opcode, Register};
use crate::stencil_plan::{fold_numeric_sources, numeric_operation, NumericSource, MAX_BLOCK_VALUES};

const MAX_VALUE_GRAPH_CAPACITY: usize = u8::MAX as usize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ValueId {
    pub register: Register,
    pub version: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ValueDefinition {
    Source(NumericSource),
    Alias(ValueId),
    NegateConstant {
        source: ValueId,
    },
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
pub(crate) struct ValueGraph<const CAPACITY: usize> {
    nodes: [ValueNode; CAPACITY],
    len: u8,
}

pub(crate) type BlockValueGraph = ValueGraph<MAX_BLOCK_VALUES>;

impl<const CAPACITY: usize> ValueGraph<CAPACITY> {
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
            nodes: [EMPTY; CAPACITY],
            len: 0,
        }
    }

    pub(crate) fn push(
        &mut self,
        instruction: Instruction,
        constant_bits: impl FnOnce(u16) -> Option<u64>,
    ) -> bool {
        if usize::from(self.len) == CAPACITY || CAPACITY > MAX_VALUE_GRAPH_CAPACITY {
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

    pub(crate) const fn len(self) -> usize {
        self.len as usize
    }

    pub(crate) fn nodes(&self) -> &[ValueNode] {
        &self.nodes[..usize::from(self.len)]
    }

    fn value_node(
        &self,
        instruction: Instruction,
        constant_bits: impl FnOnce(u16) -> Option<u64>,
    ) -> Option<ValueNode> {
        let flow = instruction.register_flow();
        flow.complete.then_some(())?;
        let id = self.next_id(flow.definition?)?;
        let definition = self.value_definition(instruction, constant_bits)?;
        Some(ValueNode { id, definition })
    }

    fn value_definition(
        &self,
        instruction: Instruction,
        constant_bits: impl FnOnce(u16) -> Option<u64>,
    ) -> Option<ValueDefinition> {
        match instruction.opcode {
            Opcode::LoadLocal if pure(instruction.opcode) => {
                Some(ValueDefinition::Source(NumericSource::Local(instruction.b)))
            }
            Opcode::LoadConst if pure(instruction.opcode) => Some(ValueDefinition::Source(
                NumericSource::Constant(constant_bits(instruction.b)?),
            )),
            Opcode::Move if instruction.flags == 0 && pure(instruction.opcode) => {
                Some(ValueDefinition::Alias(self.canonical(self.current(instruction.b)?)?))
            }
            Opcode::Unary
                if instruction.flags == crate::ir::compact_unary_id(crate::ops::UnaryOp::Minus) =>
            {
                self.negate_constant_definition(instruction)
            }
            Opcode::AddConst => self.add_constant_definition(instruction, constant_bits),
            opcode if opcode.has_guard(crate::facts::OperationGuard::Number) => {
                self.binary_definition(instruction)
            }
            _ => None,
        }
    }

    fn binary_definition(&self, instruction: Instruction) -> Option<ValueDefinition> {
        Some(ValueDefinition::Binary {
            operator: numeric_operation(instruction)?,
            lhs: self.canonical(self.current(instruction.b)?)?,
            rhs: self.canonical(self.current(instruction.c)?)?,
        })
    }

    fn negate_constant_definition(&self, instruction: Instruction) -> Option<ValueDefinition> {
        let source = self.canonical(self.current(instruction.b)?)?;
        matches!(self.resolve(source)?, NumericSource::Constant(_)).then_some(())?;
        Some(ValueDefinition::NegateConstant { source })
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

    pub(crate) fn current(&self, register: Register) -> Option<ValueId> {
        self.nodes()
            .iter()
            .rfind(|node| node.id.register == register)
            .map(|node| node.id)
    }

    pub(crate) fn node(&self, id: ValueId) -> Option<ValueNode> {
        self.nodes().iter().copied().find(|node| node.id == id)
    }

    pub(crate) fn canonical(&self, mut id: ValueId) -> Option<ValueId> {
        for _ in 0..self.len() {
            match self.node(id)?.definition {
                ValueDefinition::Alias(next) => id = next,
                _ => return Some(id),
            }
        }
        None
    }

    pub(crate) fn resolve_register(&self, register: Register) -> Option<NumericSource> {
        self.resolve(self.current(register)?)
    }

    pub(crate) fn resolve(&self, id: ValueId) -> Option<NumericSource> {
        match self.node(id)?.definition {
            ValueDefinition::Source(source) => Some(source),
            ValueDefinition::Alias(source) => self.resolve(source),
            ValueDefinition::NegateConstant { source } => self.resolve_negated(source),
            ValueDefinition::AddConstant { source, bits, left } => {
                self.resolve_add_constant(source, bits, left)
            }
            ValueDefinition::Binary { operator, lhs, rhs } => {
                let inputs = [self.resolve(lhs)?, self.resolve(rhs)?];
                fold_numeric_sources(inputs, operator).map(NumericSource::Constant)
            }
        }
    }

    fn resolve_negated(&self, source: ValueId) -> Option<NumericSource> {
        const NUMBER_SIGN_BIT: u64 = 1 << 63;
        let NumericSource::Constant(bits) = self.resolve(source)? else {
            return None;
        };
        Some(NumericSource::Constant(bits ^ NUMBER_SIGN_BIT))
    }

    fn resolve_add_constant(
        &self,
        source: ValueId,
        bits: u64,
        left: bool,
    ) -> Option<NumericSource> {
        let source = self.resolve(source)?;
        let inputs = if left {
            [NumericSource::Constant(bits), source]
        } else {
            [source, NumericSource::Constant(bits)]
        };
        fold_numeric_sources(inputs, crate::ops::BinaryOp::Add).map(NumericSource::Constant)
    }
}

fn pure(opcode: Opcode) -> bool {
    opcode.effects() == &[crate::facts::OperationEffect::Pure]
}
