fn execute_plan_loop(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    let Some(crate::facts::CountedMethodLoopFact::Visit {
        length_method,
        element_method,
        body_method,
    }) = function.code.facts().counted_method_loop.as_deref()
    else {
        return Ok(None);
    };
    let mut index = 0.0;
    loop {
        let size = call_fact_named(receiver, length_method, &[])?;
        let condition = crate::vm::vm_arithmetic::evaluate_binary(
            &crate::value::Value::Number(index),
            &size,
            crate::ops::BinaryOp::LessThan,
        )?;
        if !crate::vm::is_truthy(&condition) {
            break;
        }
        let constraint = call_fact_named(
            receiver,
            element_method,
            &[crate::value::Value::Number(index)],
        )?;
        let constraint = crate::locals::resolved_replacement(constraint);
        let body = crate::execute::get_property_result(&constraint, body_method)?;
        if execute_direct_counted_method(&body, &constraint).is_none() {
            let _ = crate::functions::execute_target(&body, &constraint, &[])?;
        }
        crate::execution_trace::kernel("plan_execute_loop", false);
        index += 1.0;
    }
    Ok(Some(crate::value::Value::Undefined))
}

fn execute_direct_counted_method(
    callee: &crate::value::Value,
    receiver: &crate::value::Value,
) -> Option<()> {
    let crate::value::Value::Function(function) = callee else {
        return None;
    };
    match function.code.facts().direct_method.as_deref()? {
        crate::facts::DirectMethodFact::Noop => {
            crate::execution_trace::kernel("counted_method_noop", false);
            Some(())
        }
        crate::facts::DirectMethodFact::CopyMethodProperty {
            target_method,
            source_method,
            property,
        } => {
            let target = direct_property_select(function, receiver, target_method)?;
            let source = direct_property_select(function, receiver, source_method)?;
            let crate::value::Value::Object(target) = target else {
                return None;
            };
            let crate::value::Value::Object(source) = source else {
                return None;
            };
            let target = plain_own_word(&target, property)?;
            let source = crate::vm::proven_own_word(&source, property)?;
            target.copy_from(source);
            crate::execution_trace::kernel("counted_method_copy_property", false);
            Some(())
        }
        crate::facts::DirectMethodFact::PropertyLoad { property } => {
            let _ = crate::execute::get_property_result(receiver, property).ok()?;
            Some(())
        }
        crate::facts::DirectMethodFact::PropertyNotEqualCapture {
            property,
            capture_slot,
            capture_property,
        } => {
            let _ = direct_not_equal_capture(
                function,
                receiver,
                property,
                *capture_slot,
                capture_property,
            )?;
            Some(())
        }
        crate::facts::DirectMethodFact::AppendArray { .. } => None,
    }
}

fn direct_not_equal_capture(
    function: &crate::value::FunctionValue,
    receiver: &crate::value::Value,
    property: &str,
    capture_slot: u16,
    capture_property: &str,
) -> Option<bool> {
    let left = plain_data_property(receiver, property)?;
    let capture = function.captures.get(capture_slot);
    let right = plain_data_property(&capture, capture_property)?;
    Some(!crate::equality::abstract_equal(&left, &right).ok()?)
}

fn direct_property_select(
    _: &crate::value::FunctionValue,
    receiver: &crate::value::Value,
    method: &str,
) -> Option<crate::value::Value> {
    let method = crate::execute::get_property_result(receiver, method).ok()?;
    let crate::value::Value::Function(method) = method else {
        return None;
    };
    let ShapeKernelPlan::PropertySelect(plan) = shape_kernel_fact(&method)? else {
        return None;
    };
    execute_property_select(&method, receiver, plan)
}

fn plain_own_word<'a>(
    object: &'a crate::value::ObjectData,
    property: &str,
) -> Option<&'a crate::register_file::SlotWord> {
    if object.hot_properties().names().any(|name| {
        crate::builtins::is_deleted_key_for(name, property)
            || crate::builtins::is_descriptor_key_for(name, property)
    }) {
        return None;
    }
    crate::vm::proven_own_word(object, property)
}

fn call_fact_named(
    receiver: &crate::value::Value,
    name: &str,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let callee = crate::execute::get_property_result(receiver, name)?;
    crate::functions::execute_target(&callee, receiver, arguments)
}

fn execute_constraint_collection_loop(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    arguments: &[crate::value::Value],
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    let Some(crate::facts::CountedMethodLoopFact::Filter {
        determining_property,
        collection_property,
        length_method,
        element_method,
        predicate_method,
        append_method,
    }) = function.code.facts().counted_method_loop.as_deref()
    else {
        return Ok(None);
    };
    let Some(variable) = arguments.first() else {
        return Ok(None);
    };
    let Some(collection) = arguments.get(1) else {
        return Ok(None);
    };
    let variable = crate::locals::resolved_replacement(variable.clone());
    let collection = crate::locals::resolved_replacement(collection.clone());
    macro_rules! admit {
        ($value:expr, $reason:literal) => {
            match $value {
                Some(value) => value,
                None => {
                    crate::execution_trace::kernel(concat!("filtered_reject_", $reason), true);
                    return Ok(None);
                }
            }
        };
    }
    let determining = admit!(
        plain_data_property(&variable, determining_property),
        "determining"
    );
    let constraints = admit!(
        plain_data_property(&variable, collection_property),
        "collection"
    );
    let length = admit!(plain_method(&constraints, length_method), "length_method");
    let element = admit!(plain_method(&constraints, element_method), "element_method");
    let input = admit!(
        nested_array_for_methods(&constraints, &length, &element),
        "input_array"
    );
    let append = admit!(plain_method(&collection, append_method), "append_method");
    let Some(crate::facts::DirectMethodFact::AppendArray { property }) =
        append.code.facts().direct_method.as_deref()
    else {
        crate::execution_trace::kernel("filtered_reject_append_fact", true);
        return Ok(None);
    };
    let Some(crate::value::Value::Array(output)) = plain_data_property(&collection, property)
    else {
        crate::execution_trace::kernel("filtered_reject_output_array", true);
        return Ok(None);
    };
    if !input.is_packed_ordinary()
        || !output.is_packed_ordinary()
        || !crate::locals::array_word_is_current(&input)
        || !crate::locals::array_word_is_current(&output)
        || std::rc::Rc::ptr_eq(&input, &output)
    {
        crate::execution_trace::kernel("filtered_reject_array_guard", true);
        return Ok(None);
    }

    // Admission above proves the collection plumbing once. Predicate guards
    // remain per element because the input may legally contain several
    // ordinary object shapes; no output is mutated until every guard passes.
    let mut selected = Vec::new();
    let iterations = input.logical_len();
    for index in 0..iterations {
        let Some(constraint) = input.dense_value_at(index) else {
            crate::execution_trace::kernel("filtered_reject_input_hole", true);
            return Ok(None);
        };
        let constraint = crate::locals::resolved_replacement(constraint);
        if crate::equality::abstract_equal(&constraint, &determining)? {
            continue;
        }
        let Some(predicate) = plain_method(&constraint, predicate_method) else {
            crate::execution_trace::kernel("filtered_reject_predicate_method", true);
            return Ok(None);
        };
        let Some(satisfied) = execute_direct_predicate(&predicate, &constraint) else {
            crate::execution_trace::kernel("filtered_reject_predicate_fact", true);
            return Ok(None);
        };
        if satisfied {
            selected.push(constraint);
        }
    }
    if !selected.is_empty() {
        let receiver = crate::value::Value::Array(output);
        let _ = crate::builtins::array_push(Some(&receiver), &selected);
    }
    for _ in 0..iterations {
        crate::execution_trace::kernel("constraint_collection_loop", false);
        crate::execution_trace::kernel("filtered_method_loop_direct", false);
    }
    Ok(Some(crate::value::Value::Undefined))
}

fn nested_array_for_methods(
    receiver: &crate::value::Value,
    length: &std::rc::Rc<crate::value::FunctionValue>,
    element: &std::rc::Rc<crate::value::FunctionValue>,
) -> Option<std::rc::Rc<crate::value::ArrayData>> {
    let ShapeKernelPlan::NestedArrayLength(length_plan) = shape_kernel_fact(length)? else {
        return None;
    };
    let ShapeKernelPlan::NestedArrayIndex(element_plan) = shape_kernel_fact(element)? else {
        return None;
    };
    let length_property = length
        .code
        .code()?
        .metadata_at(length_plan.first_pc)?
        .name
        .as_deref()?;
    let element_property = element
        .code
        .code()?
        .metadata_at(element_plan.first_pc)?
        .name
        .as_deref()?;
    if length_property != element_property {
        return None;
    }
    let crate::value::Value::Array(array) = plain_data_property(receiver, length_property)? else {
        return None;
    };
    Some(array)
}

fn execute_direct_predicate(
    function: &crate::value::FunctionValue,
    receiver: &crate::value::Value,
) -> Option<bool> {
    match function.code.facts().direct_method.as_deref()? {
        crate::facts::DirectMethodFact::PropertyLoad { property } => {
            plain_data_property(receiver, property).map(|value| crate::vm::is_truthy(&value))
        }
        crate::facts::DirectMethodFact::PropertyNotEqualCapture {
            property,
            capture_slot,
            capture_property,
        } => direct_not_equal_capture(
            function,
            receiver,
            property,
            *capture_slot,
            capture_property,
        ),
        _ => None,
    }
}

fn plain_method(
    receiver: &crate::value::Value,
    name: &str,
) -> Option<std::rc::Rc<crate::value::FunctionValue>> {
    let crate::value::Value::Function(function) = plain_data_property(receiver, name)? else {
        return None;
    };
    Some(function)
}

fn plain_data_property(receiver: &crate::value::Value, name: &str) -> Option<crate::value::Value> {
    let mut owner = crate::locals::resolved_replacement(receiver.clone());
    for _ in 0..4 {
        let crate::value::Value::Object(object) = &owner else {
            return None;
        };
        if object.has_replacement() {
            return None;
        }
        let shadows = object.hot_properties().names().any(|candidate| {
            candidate == name
                || crate::builtins::is_deleted_key_for(candidate, name)
                || crate::builtins::is_descriptor_key_for(candidate, name)
        });
        if shadows {
            return crate::vm::proven_own_data(&owner, name)
                .map(crate::locals::resolved_replacement);
        }
        owner = match crate::vm::proven_own_data(&owner, "\0prototype")? {
            crate::value::Value::ObjectAlias(alias) => crate::value::Value::Object(alias.target()?),
            prototype => crate::locals::resolved_replacement(prototype),
        };
    }
    None
}
