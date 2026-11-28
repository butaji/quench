//! The operand classes of the operator sites, from the solver: the type the function's one settled
//! solve gives each register just before a site (`FunctionFacts::cell_at`), never what a run met.
//! A specializer chooses its variant from the class: an int32 row where both operands are int32s, a
//! Number row where both are Numbers, the generic row otherwise. A function with no facts has
//! `Type::ANY` for every cell, so its sites are `Any`.

use quench_runtime::bytecode::WideInstruction;
use quench_runtime::compile::aot_facts::AotFacts;
use quench_runtime::staged::meaning::{means, Meaning};
use quench_runtime::typed_ir::Type;
use quench_runtime::ValueType;

/// The class of a site's operands, ordered by how much its specialization must allow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OperandClass {
    /// Both operands are int32s.
    Int32,
    /// Both operands are Numbers, one at least no int32.
    Number,
    /// Any other operands.
    Any,
}

/// What the solver proves of a site's operands: their class, and whether one is a string (a `+` then concatenates).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Site {
    pub(crate) class: OperandClass,
    pub(crate) string: bool,
}

impl Site {
    /// The site of two operands of the types `types`.
    pub(crate) fn of(types: [Type; 2]) -> Self {
        Self { class: class_of(types), string: types.iter().any(|ty| string_cell(*ty)) }
    }
}

/// The class of two operands of the types `types`.
pub(crate) fn class_of(types: [Type; 2]) -> OperandClass {
    if types.iter().all(|ty| int32_cell(*ty)) {
        OperandClass::Int32
    } else if types.iter().all(|ty| number_cell(*ty)) {
        OperandClass::Number
    } else {
        OperandClass::Any
    }
}

/// Whether the site of `instruction` chooses its variant from its operand classes: a binary operator record (`Binary`,
/// the Number forms `NumericAdd` and `NumericMultiply`) or a compare jump.
pub(crate) fn chooses_variant(instruction: WideInstruction) -> bool {
    means(instruction, Meaning::Binary) || means(instruction, Meaning::NumericArithmetic) || instruction.op().compares_and_jumps()
}

/// The type of register `register` just before the instruction at `pc` of `function` (`FunctionFacts::cell_at`). A
/// register with no cell, and every register of a function without facts, is `Type::ANY`.
pub(crate) fn operand_type(facts: Option<&AotFacts>, function: u32, pc: u32, register: u16) -> Type {
    facts.and_then(|facts| facts.function_facts(function).cell_at(pc, register)).map_or(Type::ANY, |cell| cell.ty)
}

/// Whether a cell type holds int32s only. `NEVER` (a path leaves the register unset) is a subtype of every type, so it is
/// excluded: an unset register holds no value the instance may read untagged.
pub(crate) fn int32_cell(ty: Type) -> bool {
    ty != Type::NEVER && ty.is_subtype(Type::INT32)
}

/// Whether an int32 operation's result cell cannot leave int32: an int32, or a Boolean, which a relation always gives
/// (`NEVER` excluded, as `int32_cell`).
pub(crate) fn int32_result_cell(ty: Type) -> bool {
    int32_cell(ty) || (ty != Type::NEVER && ty.is_subtype(Type::language(ValueType::Boolean)))
}

/// Whether a cell type holds Numbers only, int32s or doubles (`NEVER` excluded, as `int32_cell`).
pub(crate) fn number_cell(ty: Type) -> bool {
    ty != Type::NEVER && ty.is_subtype(Type::NUMBER)
}

/// Whether a cell type holds objects only (`NEVER` excluded, as `int32_cell`).
pub(crate) fn object_cell(ty: Type) -> bool {
    ty != Type::NEVER && ty.is_subtype(Type::language(ValueType::Object))
}

/// Whether a cell type holds strings only (`NEVER` excluded, as `int32_cell`).
fn string_cell(ty: Type) -> bool {
    ty != Type::NEVER && ty.is_subtype(Type::language(ValueType::String))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_int32s_are_an_int32_site_and_a_double_makes_a_number_site() {
        assert_eq!(class_of([Type::INT32, Type::INT32]), OperandClass::Int32);
        assert_eq!(class_of([Type::INT32, Type::NUMBER]), OperandClass::Number);
        assert_eq!(class_of([Type::INT32, Type::ANY]), OperandClass::Any);
    }

    #[test]
    fn a_site_without_facts_is_any_and_an_unset_operand_is_no_int32() {
        assert_eq!(class_of([Type::ANY, Type::ANY]), OperandClass::Any);
        assert_eq!(class_of([Type::NEVER, Type::INT32]), OperandClass::Any);
        assert!(Site::of([Type::ANY, Type::language(ValueType::String)]).string);
    }
}
