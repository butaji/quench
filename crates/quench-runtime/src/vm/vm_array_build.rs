fn execute_array_plan(
    registers: &mut Vec<Value>,
    dst: u16,
    elements: &[crate::ops::ArrayElement],
) -> Result<(), VmError> {
    use crate::ops::ArrayElement;
    let mut values = Vec::new();
    let mut holes = Vec::new();
    for element in elements {
        match element {
            ArrayElement::Value(src) => values.push(read_register(registers, *src)?),
            ArrayElement::Elision => {
                holes.push(values.len());
                values.push(Value::Undefined);
            }
            ArrayElement::Spread(src) => values.extend(collect_spread(registers, *src)?),
        }
    }
    let mut array = crate::value::ArrayData::new(values);
    for index in holes {
        array.delete_property(&index.to_string());
    }
    write_value(registers, dst, Value::Array(Rc::new(array)));
    Ok(())
}

fn collect_spread(registers: &[Value], src: u16) -> Result<Vec<Value>, VmError> {
    crate::collections::iterator::collect_iterable(read_register(registers, src)?)
}
