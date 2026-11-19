//! The derivations a rewrite reads from the catalog: the one op a role names (`the_op`), the
//! jump and store twins of a row (`negated_jump`, `strict_variant`), and per-op tables built
//! from rows (`op_table!`, `row_ops!`). Every set is a `const` over the rows, so no list of
//! opcodes sits beside the table.
use super::{InstructionField, Op, OPCODE_COUNT, Traits};

/// The one op a role trait names. A rewrite spells its ops through these roles, never by name, so
/// each role must be carried by exactly one row: a second carrier fails the compile-time evaluation.
pub(crate) const fn the_op(role: Traits) -> Op {
    let mut found = None;
    let mut index = 0;
    while index < Op::COUNT {
        if let Some(op) = Op::from_index(index) {
            if op.traits().has(role) {
                assert!(found.is_none(), "a role trait names exactly one op");
                found = Some(op);
            }
        }
        index += 1;
    }
    match found {
        Some(op) => op,
        None => panic!("a role trait names exactly one op"),
    }
}

/// Whether two rows share one encoding: control flow, immediate role and all three operand layouts.
const fn same_encoding(a: Op, b: Op) -> bool {
    a.control_flow_layout() as u8 == b.control_flow_layout() as u8
        && a.immediate_role() as u8 == b.immediate_role() as u8
        && a.field_layout(InstructionField::A) as u8 == b.field_layout(InstructionField::A) as u8
        && a.field_layout(InstructionField::B) as u8 == b.field_layout(InstructionField::B) as u8
        && a.field_layout(InstructionField::C) as u8 == b.field_layout(InstructionField::C) as u8
}

/// The jump that tests the opposite truth of `op`: the row whose traits are `op`'s with the
/// `JUMPS_ON_FALSE` and `JUMPS_ON_TRUE` bits swapped, sharing its encoding. `None` for any other op.
pub(crate) const fn negated_jump(op: Op) -> Option<Op> {
    let traits = op.traits().0;
    let (on_false, on_true) = (Traits::JUMPS_ON_FALSE.0, Traits::JUMPS_ON_TRUE.0);
    if traits & (on_false | on_true) == 0 {
        return None;
    }
    let swapped = (traits & !(on_false | on_true))
        | if traits & on_false != 0 { on_true } else { 0 }
        | if traits & on_true != 0 { on_false } else { 0 };
    let mut found = None;
    let mut index = 0;
    while index < Op::COUNT {
        if let Some(candidate) = Op::from_index(index) {
            if candidate.traits().0 == swapped && candidate.traits().1 == op.traits().1 && same_encoding(candidate, op) {
                assert!(found.is_none(), "a jump has one negation");
                found = Some(candidate);
            }
        }
        index += 1;
    }
    found
}

/// The strict form of a `SLOPPY_STORE` row: the `STRICT_STORE` row sharing its encoding. `None` for any other op.
pub(crate) const fn strict_variant(op: Op) -> Option<Op> {
    if !op.traits().has(Traits::SLOPPY_STORE) {
        return None;
    }
    let mut found = None;
    let mut index = 0;
    while index < Op::COUNT {
        if let Some(candidate) = Op::from_index(index) {
            if candidate.traits().has(Traits::STRICT_STORE) && same_encoding(candidate, op) {
                assert!(found.is_none(), "a sloppy store has one strict form");
                found = Some(candidate);
            }
        }
        index += 1;
    }
    found
}

/// A per-op table, indexed by `Op as usize`, computed from each op's row. `op_table!(default, op => row)`
/// evaluates `row` for every op in the catalog, so no table lists ops by hand.
macro_rules! op_table {
    ($default:expr, $op:ident => $row:expr) => {{
        let mut table = [$default; $crate::bytecode::OPCODE_COUNT];
        let mut index = 0;
        while index < $crate::bytecode::OPCODE_COUNT {
            if let Some($op) = $crate::bytecode::Op::from_index(index) {
                table[index] = $row;
            }
            index += 1;
        }
        table
    }};
}

/// A per-op set: `true` where the row predicate holds.
pub(crate) type OpSet = [bool; OPCODE_COUNT];

/// Builds an `OpSet` from a predicate over each op's rows. The predicate is a `const` expression,
/// so every set is fixed at compile time and no list of ops can fall behind the rows.
macro_rules! row_ops {
    ($op:ident => $row:expr) => {
        $crate::bytecode::op_table!(false, $op => $row)
    };
}

/// Roles emitted by more than one compile file. Each names the one op its trait selects.
pub(crate) const REGISTER_MOVE: Op = the_op(Traits::REGISTER_COPY);
pub(crate) const JUMP_IF_FALSE: Op = the_op(Traits::JUMPS_ON_FALSE);
pub(crate) const CALL_INVOKE: Op = the_op(Traits::CALL_INVOKE);
pub(crate) const ELEMENT_STORE: Op = the_op(Traits::ELEMENT_STORE);
pub(crate) const OBJECT_FIELD_STORE: Op = the_op(Traits::OBJECT_FIELD_STORE);
pub(crate) const RETURN_OP: Op = the_op(Traits::RETURN_VALUE);

pub(crate) use op_table;
pub(crate) use row_ops;

#[cfg(test)]
mod role_tests {
    use super::*;

    /// Each role names the op it replaced in the emitters, and no other row carries its trait.
    #[test]
    fn roles_select_the_ops_they_replaced() {
        assert_eq!(the_op(Traits::TDZ_INITIALIZE), Op::InitializeTdz);
        assert_eq!(the_op(Traits::CLASS_HERITAGE_CHECK), Op::ValidateClassHeritage);
        assert_eq!(the_op(Traits::MAKES_CLOSURE), Op::MakeClosure);
        assert_eq!(the_op(Traits::FIELD_LEAF), Op::GetField);
        assert_eq!(the_op(Traits::PRIVATE_NAME_MARK), Op::MarkPrivateName);
        assert_eq!(the_op(Traits::JUMPS_ON_FALSE), Op::JumpFalse);
        assert_eq!(the_op(Traits::UNCONDITIONAL_JUMP), Op::Jump);
        assert_eq!(the_op(Traits::REGISTER_COPY), Op::Move);
        assert_eq!(the_op(Traits::PROPERTY_KEY_CONVERT), Op::ToPropertyKey);
        assert_eq!(the_op(Traits::FUNCTION_NAME_KEY_SET), Op::SetFunctionNameKey);
        assert_eq!(the_op(Traits::CALL_INVOKE), Op::Call);
        assert_eq!(the_op(Traits::ELEMENT_STORE), Op::SetIndex);
        assert_eq!(the_op(Traits::OBJECT_FIELD_STORE), Op::SetField);
        assert_eq!(the_op(Traits::PRIVATE_FIELD_DEFINE), Op::DefinePrivateField);
        assert_eq!(the_op(Traits::COMPUTED_FIELD_DEFINE), Op::DefineComputedField);
        assert_eq!(the_op(Traits::FIELD_DEFINE), Op::DefineField);
        assert_eq!(the_op(Traits::LOCAL_BINDING_STORE), Op::StoreLocal);
        assert_eq!(the_op(Traits::PROPERTY_RECORD_DEFINE), Op::DefinePropertyRecord);
        assert_eq!(the_op(Traits::THIS_LEAF), Op::LoadThis);
        assert_eq!(the_op(Traits::TEMPLATE_CACHE_LOAD), Op::LoadCachedTemplateObject);
        assert_eq!(the_op(Traits::TEMPLATE_CACHE_STORE), Op::CacheTemplateObject);
        assert_eq!(the_op(Traits::ARRAY_MAKE), Op::MakeArray);
        assert_eq!(the_op(Traits::OBJECT_MAKE), Op::MakeObject);
        assert_eq!(the_op(Traits::RETURNING_UNARY), Op::Unary);
        assert_eq!(the_op(Traits::CONSTANT_LOAD), Op::LoadConst);
        assert_eq!(the_op(Traits::STRING_CONCAT), Op::StringConcat);
        assert_eq!(the_op(Traits::RETURN_VALUE), Op::Return);
        assert_eq!(the_op(Traits::FIRST_CALL_CHUNK), Op::CallFirstChunk);
    }
}
