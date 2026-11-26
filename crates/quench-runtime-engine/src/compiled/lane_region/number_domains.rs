//! The representation a Number operation of the lane body runs in, chosen from its operands'
//! proved facts.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// The machine view a block entry keeps a register of fact type `ty` in: the unboxed view of
/// a Number type, tagged bits for any other. Entry states and edges both take it from the
/// target's fact (`facts.at`), so an edge conforms to exactly what its target reads.
pub(crate) fn target_representation(ty: Type) -> Representation {
    let ty = ty.machine();
    if ty == Type::INT32 { Representation::Int32 }
    else if ty == Type::NUMBER { Representation::Float64 }
    else { Representation::Tagged }
}

/// `index`'s tagged bits boxed from its unboxed view in `representation`: the one boxing of a
/// numeric register, for a tagged read (`frame::register_tagged`) and for a leave (`exits.rs`).
pub(crate) fn box_unboxed(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    representation: Representation, index: usize) -> ir::Value {
    match representation {
        Representation::Int32 => {
            let variable = frame::register_int(builder, registers, index);
            let int = builder.use_var(variable);
            int_tagged(builder, int)
        }
        Representation::Int64 => {
            let variable = frame::register_long(builder, registers, index);
            let word = builder.use_var(variable);
            let number = quench_runtime::staged::Float64::of_long(&mut super::object_slots::emit(builder), word);
            emit_js_box_number(builder, number)
        }
        Representation::Float64 => {
            let variable = frame::register_float(builder, registers, index);
            let number = quench_runtime::staged::Float64::by_fact(builder.use_var(variable), quench_runtime::staged::NumberFact::float64_home());
            emit_js_box_number(builder, number)
        }
        Representation::Tagged => unreachable!("a tagged register's bits are current"),
    }
}

impl Lowering<'_, '_> {
    /// Whether every operand is proved int32: a register by its type, a constant by its value.
    pub(crate) fn proved_int(&self, state: &State, operands: [Operand; 2]) -> bool {
        operands.iter().all(|operand| match operand.kind() {
            Some(OperandKind::Register) => state.values[usize::from(operand.payload())].value.ty.is_subtype(Type::INT32),
            Some(OperandKind::Constant) => self.program.constants.get(usize::from(operand.payload()))
                .and_then(super::super::facts::immediate).is_some_and(super::super::facts::is_int32),
            _ => false,
        })
    }

    /// `left % right` into `destination` at `pc`, both operands Numbers. Operands proved int32 take
    /// the int32 remainder, as C does; its NaN and `-0` leave at `exit`. The guards the facts prove
    /// redundant are dropped: a dividend no negative makes no `-0`, a sum that only its ToInt32 reads
    /// ignores the sign of one (`truncated_sum_operand`), and a divisor no zero needs no zero test.
    /// The Number remainder: whether the body still falls through. A linking lowering leaves the
    /// record instead (an AOT image must not embed the compiling process's remainder address), so
    /// the caller stops lowering the block it just terminated.
    pub(crate) fn remainder(&mut self, state: &mut State, pc: usize, destination: Register,
        [left, right]: [Operand; 2], exit: ir::Block) -> bool {
        // A binary64 remainder is a leaf call whose target the staged row embeds as its process
        // address (`number_machine::float_remainder`). Linking code (an AOT image) must not embed
        // one: the address is the compiling process's, so the record leaves and the lane computes
        // it (the image's `wrapUp` otherwise branched into that process's binary at load).
        if self.helpers.links() {
            self.exit(state, pc as u32);
            return false;
        }
        let int = self.proved_int(state, [left, right]) && !self.left_here;
        let domain = if int {
            quench_runtime::staged::Domain::Int32
        } else {
            quench_runtime::staged::Domain::Float64
        };
        let (zero, divisor_nonzero) = if int {
            let sign_unread = self.ranges.non_negative_dividend(pc) || self.truncated_sum_operand(pc, destination);
            let zero = if sign_unread { quench_runtime::staged::NegativeZero::Ignore } else { quench_runtime::staged::NegativeZero::Exit };
            (zero, self.ranges.nonzero_divisor(pc))
        } else {
            (quench_runtime::staged::NegativeZero::Exit, false)
        };
        let operation = quench_runtime::staged::Operation::Number(quench_runtime::staged::NumberOperation::Remainder {
            destination, left, right, domain, zero, divisor_nonzero });
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
        let other = Type::language(quench_runtime::ValueType::BigInt);
        assert_eq!(target_representation(Type::INT32.join(other)), Representation::Tagged);
        assert_eq!(target_representation(Type::NUMBER.join(Type::ANY)), Representation::Tagged);
    }
}
