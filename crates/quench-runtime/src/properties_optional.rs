pub(crate) fn execute_optional_get(
    registers: &mut Vec<crate::value::Value>,
    op: &crate::ops::Op,
) -> Result<(), crate::execute::VmError> {
    let crate::ops::Op::OptionalGet { dst, object, key } = op else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let object = crate::execute::read_register(registers, *object)?;
    let value = if matches!(object, crate::value::Value::Null | crate::value::Value::Undefined) {
        crate::value::Value::Undefined
    } else {
        crate::execute::get_property_result(&object, key)?
    };
    crate::execute::write_value(registers, *dst, value);
    Ok(())
}
