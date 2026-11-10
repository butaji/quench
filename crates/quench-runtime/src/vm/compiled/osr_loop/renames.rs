//! Renaming an inlined callee's operands into the region's registers.
use super::*;

/// `renamed`'s field rename: `raw` through `register`, a `this` field base made `this`, and a
/// construction's argument window renamed as a call's.
pub(super) fn field(op: Op, field: InstructionField, raw: u16, imm: u32, this: Register,
    register: &dyn Fn(Register) -> Register) -> Option<u16> {
    match op.field_layout(field) {
        FieldLayout::Unused
        | FieldLayout::BooleanFlag
        | FieldLayout::BinaryOperator
        | FieldLayout::CacheSiteIndex
        | FieldLayout::FieldLookupCacheSiteIndex => Some(raw),
        FieldLayout::ResultRegister | FieldLayout::Register | FieldLayout::WriteRegister | FieldLayout::ReadWriteRegister =>
            (raw <= Operand::PAYLOAD_MASK).then(|| register(raw)),
        FieldLayout::Operand | FieldLayout::NumericIndexOperand => operand(raw, register),
        FieldLayout::FieldBase => field_base(raw, this, register),
        FieldLayout::ConstructArguments => construct_arguments(raw, imm, register),
        _ => None,
    }
}

/// An operand field: a register through `register`, a constant as it is, and no field or
/// local, which an inlined body does not name by register.
fn operand(raw: u16, register: &dyn Fn(Register) -> Register) -> Option<u16> {
    let operand = Operand(raw);
    match operand.kind()? {
        OperandKind::Register => Some(Operand::register(register(operand.payload())).0),
        OperandKind::Constant => Some(raw),
        OperandKind::Field | OperandKind::Local => None,
    }
}

/// A field base: `this` for a `this` base, else the base's register through `register`.
fn field_base(raw: u16, this: Register, register: &dyn Fn(Register) -> Register) -> Option<u16> {
    let base = FieldBase(raw);
    Some(FieldBase::register(if base == FieldBase::THIS { this } else { register(base.register_index()?) }).0)
}

/// The `Construct` field C of a construction reading `raw`'s window, renamed: its base
/// through `register`, and each window register to the one after the base's, as a call's
/// window renames; an array-argument construction renames only its array's register.
pub(super) fn construct_arguments(raw: u16, imm: u32, register: &dyn Fn(Register) -> Register) -> Option<u16> {
    let base = Register::try_from(raw).ok()?;
    let renamed = register(base);
    if !ImmediateLayout::construct_array_arguments(imm) {
        let count = ImmediateLayout::argument_count(imm);
        (0..count).all(|offset| register(base + offset) == renamed + offset).then_some(())?;
    }
    Some(renamed)
}
