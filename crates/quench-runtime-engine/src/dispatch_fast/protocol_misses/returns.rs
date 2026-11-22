//! Protocol misses of returns and returning records (`protocol_family::RETURN`).

use super::super::*;

/// Kinds of the RETURN family's parameter codes: a returning record's kind (`RETURNING_*`, 0..=12),
/// then a `Return` (`RETURN_KIND`) and a `LoadConst` fused into its `Return` (`CONSTANT_KIND`).
pub(in crate::dispatch_fast) const RETURN_KIND: u8 = 13;
pub(in crate::dispatch_fast) const CONSTANT_KIND: u8 = 14;
/// Leaf forms a return record takes (`LeafForm`, 0..=4).
pub(in crate::dispatch_fast) const LEAF_FORMS: u8 = 5;
/// Kinds of a returning constant-right `Binary` (`returning_operator_row`): `OPERATOR_KIND` plus the
/// operator's `BinaryOperator` value, so each operator is its own kind (values up to 21 fit the byte).
pub(in crate::dispatch_fast) const OPERATOR_KIND: u8 = 15;

/// The RETURN family's parameter code of a record of `kind` at leaf form `leaf`:
/// `kind * LEAF_FORMS + leaf`, which fits the byte for every kind above.
pub(in crate::dispatch_fast) const fn returning_code(kind: u8, leaf: u8) -> u8 {
    kind * LEAF_FORMS + leaf
}

/// The RETURN family's parameter of a returning constant-right `Binary` of operator `operator` at leaf
/// form `leaf` (`returning_operator_row`'s MISS).
pub(in crate::dispatch_fast) const fn operator_return_code(operator: u8, leaf: u8) -> u16 {
    protocol_family::RETURN | (returning_code(OPERATOR_KIND + operator, leaf) as u16)
}

/// A `match` on `$leaf` (0..=4) that binds `$l` to the const leaf form, as `by_leaf!` does for a
/// const generic: here for a runtime parameter, so each arm is its own instantiation.
macro_rules! leaf_arms {
    ($leaf:expr, |$l:ident| $body:expr) => {
        match $leaf {
            0 => { const $l: u8 = 0; $body }
            1 => { const $l: u8 = 1; $body }
            2 => { const $l: u8 = 2; $body }
            3 => { const $l: u8 = 3; $body }
            _ => { const $l: u8 = 4; $body }
        }
    };
}

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row::<OP, ACC, { protocol_family::RETURN | parameters }>`), where `parameters` is
/// `returning_code(kind, leaf)`.
pub(in crate::dispatch_fast) const fn return_miss_of(parameters: u8) -> Handler {
    let kind = parameters / LEAF_FORMS;
    let leaf = parameters % LEAF_FORMS;
    match kind {
        RETURN_KIND => leaf_arms!(leaf, |L| return_miss::<L> as Handler),
        CONSTANT_KIND => leaf_arms!(leaf, |L| return_constant_miss::<L> as Handler),
        RETURNING_FIELD => leaf_arms!(leaf, |L| returning_miss::<RETURNING_FIELD, L> as Handler),
        RETURNING_FIELD_OWN => leaf_arms!(leaf, |L| returning_miss::<RETURNING_FIELD_OWN, L> as Handler),
        RETURNING_FIELD_POLYMORPHIC => leaf_arms!(leaf, |L| returning_miss::<RETURNING_FIELD_POLYMORPHIC, L> as Handler),
        RETURNING_FIELD_HOLDER => leaf_arms!(leaf, |L| returning_miss::<RETURNING_FIELD_HOLDER, L> as Handler),
        RETURNING_FIELD_INHERITED => leaf_arms!(leaf, |L| returning_miss::<RETURNING_FIELD_INHERITED, L> as Handler),
        RETURNING_FIELD_NESTED => leaf_arms!(leaf, |L| returning_miss::<RETURNING_FIELD_NESTED, L> as Handler),
        RETURNING_FIELD_PRIVATE => leaf_arms!(leaf, |L| returning_miss::<RETURNING_FIELD_PRIVATE, L> as Handler),
        RETURNING_FIELD_GETTER => leaf_arms!(leaf, |L| returning_miss::<RETURNING_FIELD_GETTER, L> as Handler),
        RETURNING_UNARY => leaf_arms!(leaf, |L| returning_miss::<RETURNING_UNARY, L> as Handler),
        RETURNING_BINARY => leaf_arms!(leaf, |L| returning_miss::<RETURNING_BINARY, L> as Handler),
        RETURNING_OBJECT => leaf_arms!(leaf, |L| returning_miss::<RETURNING_OBJECT, L> as Handler),
        RETURNING_OBJECT_LITERAL => leaf_arms!(leaf, |L| returning_miss::<RETURNING_OBJECT_LITERAL, L> as Handler),
        RETURNING_LITERAL => leaf_arms!(leaf, |L| returning_miss::<RETURNING_LITERAL, L> as Handler),
        kind if kind >= OPERATOR_KIND => leaf_arms!(leaf, |L| match returning_operator_miss_of::<L>(kind - OPERATOR_KIND) {
            Some(handler) => handler,
            None => general_step_in_lane,
        }),
        _ => general_step_in_lane,
    }
}
