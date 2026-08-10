use std::{cell::RefCell, rc::Rc};

use crate::{
    execute::VmError,
    value::{GeneratorData, Value},
};

pub(crate) fn create(
    function: &Rc<crate::value::FunctionValue>,
    receiver: &Value,
    arguments: &[Value],
) -> Value {
    Value::Generator(Rc::new(GeneratorData {
        function: Rc::clone(function),
        receiver: receiver.clone(),
        arguments: arguments.to_vec(),
        done: RefCell::new(false),
    }))
}

pub(crate) fn next(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Some(Value::Generator(generator)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Generator.next called on incompatible receiver",
        ));
    };
    let completion = resume(generator);
    if generator.function.is_async {
        return Ok(crate::promise::from_async_completion(completion));
    }
    completion
}

fn resume(generator: &GeneratorData) -> Result<Value, VmError> {
    if *generator.done.borrow() {
        return Ok(iterator_result(Value::Undefined));
    }
    *generator.done.borrow_mut() = true;
    crate::functions::execute_body(
        &generator.function,
        &generator.receiver,
        &generator.arguments,
    )
    .map(iterator_result)
}

fn iterator_result(value: Value) -> Value {
    Value::Object(Rc::new(vec![
        ("value".to_string(), value),
        ("done".to_string(), Value::Boolean(true)),
    ]))
}
