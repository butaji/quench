//! The representation a Number operation of the lane body runs in, chosen from its operands'
//! proved facts.

use super::*;

/// The machine view a block entry keeps a register of fact type `ty` in: the unboxed view of
/// a Number type, tagged bits for any other. Entry states and edges both take it from the
/// target's fact (`facts.at`), so an edge conforms to exactly what its target reads.
pub(super) fn target_representation(ty: Type) -> Representation {
    if ty == Type::INT32 { Representation::Int32 }
    else if ty == Type::NUMBER { Representation::Float64 }
    else { Representation::Tagged }
}

/// `index`'s tagged bits boxed from its unboxed view in `representation`: the one boxing of a
/// numeric register, for a tagged read (`frame::register_tagged`) and for a leave (`exits.rs`).
pub(super) fn box_unboxed(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    representation: Representation, index: usize) -> ir::Value {
    match representation {
        Representation::Int32 => {
            let variable = frame::register_int(builder, registers, index);
            let int = builder.use_var(variable);
            int_tagged(builder, int)
        }
        Representation::Float64 => {
            let variable = frame::register_float(builder, registers, index);
            let number = builder.use_var(variable);
            emit_js_box_number(builder, number)
        }
        Representation::Tagged => unreachable!("a tagged register's bits are current"),
    }
}

impl Lowering<'_, '_> {
    /// Whether every operand is proved int32: a register by its type, a constant by its value.
    pub(super) fn proved_int(&self, state: &State, operands: [Operand; 2]) -> bool {
        operands.iter().all(|operand| match operand.kind() {
            Some(OperandKind::Register) => state.values[usize::from(operand.payload())].value.ty.is_subtype(Type::INT32),
            Some(OperandKind::Constant) => self.program.constants.get(usize::from(operand.payload()))
                .and_then(super::super::facts::immediate).is_some_and(super::super::facts::is_int32),
            _ => false,
        })
    }

    /// `left % right` into `destination`, both operands Numbers. Operands proved int32 take
    /// the int32 remainder, as C does; its NaN and `-0` leave at `exit`.
    pub(super) fn remainder(&mut self, state: &mut State, destination: Register,
        [left, right]: [Operand; 2], exit: ir::Block) -> bool {
        let domain = if self.proved_int(state, [left, right]) && !self.left_here {
            crate::staged::Domain::Int32
        } else {
            crate::staged::Domain::Float64
        };
        let operation = crate::staged::Operation::Number(crate::staged::NumberOperation::Remainder {
            destination, left, right, domain });
        self.step(state, frame::Edges::ValueGuarded { miss: exit }, operation);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The facts lattice (`facts::analyze`) joins each block entry over every incoming edge,
    /// loop headers to a fixed point; the representation is its one function.
    #[test]
    fn a_join_picks_the_widest_number_domain_and_any_other_input_keeps_tagged() {
        assert_eq!(target_representation(Type::INT32.join(Type::INT32)), Representation::Int32);
        assert_eq!(target_representation(Type::INT32.join(Type::NUMBER)), Representation::Float64);
        let other = Type::language(crate::ValueType::BigInt);
        assert_eq!(target_representation(Type::INT32.join(other)), Representation::Tagged);
        assert_eq!(target_representation(Type::NUMBER.join(Type::ANY)), Representation::Tagged);
    }
}
