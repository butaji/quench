use crate::{execute::VmError, value::Value};

pub(crate) fn store(registers: &mut Vec<Value>, slot: u16, source: u16) -> Result<(), VmError> {
    crate::execute::copy_register(registers, slot, source)?;
    let value = crate::execute::read_register(registers, source)?.clone();
    for register in registers.iter() {
        if let Value::Function(function) = register {
            if let Some(capture) = function.captures.borrow_mut().get_mut(usize::from(slot)) {
                *capture = value.clone();
            }
        }
    }
    Ok(())
}
