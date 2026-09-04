fn execute_plan_loop(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    let Some(plan) = match_plan_loop(function) else { return Ok(None) };
    let mut index = 0.0;
    loop {
        let size = call_named_complete(receiver, plan.test, plan.size_pc, &[])?;
        let condition = crate::vm::vm_arithmetic::evaluate_binary(
            &crate::value::Value::Number(index),
            &size,
            crate::ops::BinaryOp::LessThan,
        )?;
        if !crate::vm::is_truthy(&condition) {
            break;
        }
        let constraint = call_registered_one(receiver, plan.body, plan.constraint_pc, index)?;
        let constraint = crate::locals::resolved_replacement(constraint);
        let _ = call_named_complete(&constraint, plan.body, plan.execute_pc, &[])?;
        crate::execution_trace::kernel("plan_execute_loop", false);
        index += 1.0;
    }
    Ok(Some(crate::value::Value::Undefined))
}

fn execute_counted_method_loop(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    let Some(fact) = function.code.facts().counted_method_loop.as_deref() else {
        return Ok(None);
    };
    if matches!(fact, crate::facts::CountedMethodLoopFact::BitCount) {
        let Some(crate::value::Value::Number(mut value)) = arguments.first().cloned() else {
            return Ok(None);
        };
        // The recognized loop only performs numeric comparison and ToInt32
        // bit operations. Keep non-number arguments on the interpreter path,
        // where observable coercion and exceptions remain intact.
        let mut count = 0_u32;
        while value > 0.0 {
            let bits = crate::vm::vm_arithmetic::numeric_to_int32(value);
            value = f64::from(bits & bits.wrapping_sub(1));
            count += 1;
        }
        return Ok(Some(crate::value::Value::Number(f64::from(count))));
    }
    let crate::facts::CountedMethodLoopFact::Visit {
        length_method,
        element_method,
        body_method,
    } = fact else {
        return Ok(None);
    };
    if let Some(result) =
        execute_direct_method_loop(receiver, length_method, element_method, body_method)
    {
        return Ok(Some(result));
    }
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
        crate::execution_trace::kernel("F|C|S", false);
        index += 1.0;
    }
    Ok(Some(crate::value::Value::Undefined))
}

fn execute_select_update_call(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    let Some(crate::facts::DirectMethodFact::SelectUpdateCall {
        input_method,
        output_method,
        namespace_slot,
        combine_method,
        receiver_value,
        input_value,
        output_value,
        input_flag,
        output_flag,
        extra_flag_objects,
        conditional_method,
    }) = function.code.facts().direct_method.as_deref()
    else {
        return Ok(None);
    };
    let Some(input) = plain_property_select(receiver, input_method) else {
        return Ok(None);
    };
    let Some(output) = plain_property_select(receiver, output_method) else {
        return Ok(None);
    };
    let Some(receiver_value) = plain_data_property(receiver, receiver_value) else {
        return Ok(None);
    };
    let Some(input_value) = plain_data_property(
        &crate::value::Value::Object(std::rc::Rc::clone(&input)),
        input_value,
    ) else {
        return Ok(None);
    };
    let namespace = function.captures.get(*namespace_slot);
    let combined = call_fact_named(&namespace, combine_method, &[receiver_value, input_value])?;

    // The weakest call is observable.  From this point onward the kernel must
    // complete through ordinary JS operations on a missed word guard rather
    // than return None and replay the method in the interpreter.
    let input_value = crate::value::Value::Object(std::rc::Rc::clone(&input));
    let mut flag = crate::vm::is_truthy(&crate::execute::get_property_result(
        &input_value,
        input_flag,
    )?);
    for property in extra_flag_objects {
        let owner = crate::execute::get_property_result(receiver, property)?;
        let value = crate::execute::get_property_result(&owner, input_flag)?;
        flag &= crate::vm::is_truthy(&value);
    }

    let mut output_object = crate::value::Value::Object(std::rc::Rc::clone(&output));
    if let (Some(value_word), Some(flag_word)) = (
        plain_own_word(&output, output_value),
        plain_own_word(&output, output_flag),
    ) {
        value_word.store(combined);
        flag_word.store(crate::value::Value::Boolean(flag));
    } else {
        output_object =
            crate::properties::assign_set_property(&output_object, output_value, combined)?;
        let _ = crate::properties::assign_set_property(
            &output_object,
            output_flag,
            crate::value::Value::Boolean(flag),
        )?;
    }
    if flag {
        let execute = crate::execute::get_property_result(receiver, conditional_method)?;
        if execute_direct_counted_method(&execute, receiver).is_none() {
            let _ = crate::functions::execute_target(&execute, receiver, &[])?;
        }
    }
    crate::execution_trace::kernel("S|C", false);
    Ok(Some(crate::value::Value::Undefined))
}

enum DirectMethodAction {
    Noop,
    Copy {
        target: *const crate::register_file::SlotWord,
        source: *const crate::register_file::SlotWord,
        _target_owner: std::rc::Rc<crate::value::ObjectData>,
        _source_owner: std::rc::Rc<crate::value::ObjectData>,
    },
}

impl DirectMethodAction {
    fn execute(&self) {
        match self {
            Self::Noop => crate::execution_trace::kernel("S|C", false),
            Self::Copy { target, source, .. } => {
                // SAFETY: admission roots both owners in this action and
                // ordinary own-slot mutation cannot relocate either word.
                unsafe { &**target }.copy_from(unsafe { &**source });
                crate::execution_trace::kernel("S|C", false);
            }
        }
        crate::execution_trace::kernel("F|C|S", false);
    }
}

fn execute_direct_method_loop(
    receiver: &crate::value::Value,
    length_method: &str,
    element_method: &str,
    body_method: &str,
) -> Option<crate::value::Value> {
    let length = plain_method(receiver, length_method)?;
    let element = plain_method(receiver, element_method)?;
    let input = collection_array_for_methods(receiver, &length, &element)?;
    if !input.is_packed_ordinary() || !crate::locals::array_word_is_current(&input) {
        return None;
    }
    let mut actions = Vec::with_capacity(input.logical_len());
    for index in 0..input.logical_len() {
        let constraint = crate::locals::resolved_replacement(input.dense_value_at(index)?);
        let body = plain_method(&constraint, body_method)?;
        actions.push(admit_direct_method_action(&body, &constraint)?);
    }
    for action in &actions {
        action.execute();
    }
    Some(crate::value::Value::Undefined)
}

fn collection_array_for_methods(
    receiver: &crate::value::Value,
    length: &std::rc::Rc<crate::value::FunctionValue>,
    element: &std::rc::Rc<crate::value::FunctionValue>,
) -> Option<std::rc::Rc<crate::value::ArrayData>> {
    if let Some(array) = nested_array_for_methods(receiver, length, element) {
        return Some(array);
    }
    let ShapeKernelPlan::ForwardZero(length_plan) = shape_kernel_fact(length)? else {
        return None;
    };
    let ShapeKernelPlan::ForwardOne(element_plan) = shape_kernel_fact(element)? else {
        return None;
    };
    let length_code = length.code.code()?;
    let element_code = element.code.code()?;
    let length_receiver = length_code
        .metadata_at(length_plan.receiver_pc)?
        .name
        .as_deref()?;
    let element_receiver = element_code
        .metadata_at(element_plan.receiver_pc)?
        .name
        .as_deref()?;
    if length_receiver != element_receiver {
        return None;
    }
    let nested = plain_data_property(receiver, length_receiver)?;
    let length_name = length_code
        .metadata_at(length_plan.call_pc)?
        .name
        .as_deref()?;
    let element_name = element_code
        .metadata_at(element_plan.callee_pc)?
        .name
        .as_deref()?;
    let nested_length = plain_method(&nested, length_name)?;
    let nested_element = plain_method(&nested, element_name)?;
    nested_array_for_methods(&nested, &nested_length, &nested_element)
}

fn admit_direct_method_action(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
) -> Option<DirectMethodAction> {
    match function.code.facts().direct_method.as_deref()? {
        crate::facts::DirectMethodFact::Noop => Some(DirectMethodAction::Noop),
        crate::facts::DirectMethodFact::CopyMethodProperty {
            target_method,
            source_method,
            property,
        } => {
            let target_owner = plain_property_select(receiver, target_method)?;
            let source_owner = plain_property_select(receiver, source_method)?;
            let target = plain_own_word(&target_owner, property).map(std::ptr::from_ref)?;
            let source =
                crate::vm::proven_own_word(&source_owner, property).map(std::ptr::from_ref)?;
            Some(DirectMethodAction::Copy {
                target,
                source,
                _target_owner: target_owner,
                _source_owner: source_owner,
            })
        }
        _ => None,
    }
}

fn plain_property_select(
    receiver: &crate::value::Value,
    method: &str,
) -> Option<std::rc::Rc<crate::value::ObjectData>> {
    let method = plain_method(receiver, method)?;
    let ShapeKernelPlan::PropertySelect(plan) = shape_kernel_fact(&method)? else {
        return None;
    };
    let crate::value::Value::Object(selected) = execute_property_select(&method, receiver, plan)?
    else {
        return None;
    };
    Some(selected)
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
            crate::execution_trace::kernel("S|C", false);
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
            crate::execution_trace::kernel("S|C", false);
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
        crate::facts::DirectMethodFact::AppendArray { .. }
        | crate::facts::DirectMethodFact::SlotDot3 { .. }
        | crate::facts::DirectMethodFact::SelectUpdateCall { .. } => None,
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
    if let crate::value::Value::Function(function) = &callee {
        if let Some(result) =
            crate::functions::try_execute_specialized(function, receiver, arguments)?
        {
            crate::execution_trace::kernel("CallKnown", false);
            return Ok(result);
        }
    }
    crate::functions::execute_target(&callee, receiver, arguments)
}

fn execute_constraint_collection_loop(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    arguments: &[crate::value::Value],
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    let Some(plan) = match_constraint_collection_loop(function) else { return Ok(None) };
    let Some(variable) = arguments.first() else { return Ok(None) };
    let Some(collection) = arguments.get(1) else { return Ok(None) };
    let variable = crate::locals::resolved_replacement(variable.clone());
    let collection = crate::locals::resolved_replacement(collection.clone());
    let determining = get_named_complete(&variable, plan.main, plan.determining_pc)?;
    let constraints = get_named_complete(&variable, plan.main, plan.constraints_pc)?;
    let mut index = 0.0;
    loop {
        let size = call_named_complete(&constraints, plan.test, plan.size_pc, &[])?;
        let condition = crate::vm::vm_arithmetic::evaluate_binary(
            &crate::value::Value::Number(index),
            &size,
            crate::ops::BinaryOp::LessThan,
        )?;
        if !crate::vm::is_truthy(&condition) {
            break;
        }
        let constraint = call_registered_one(
            &constraints,
            plan.body,
            plan.constraint_pc,
            index,
        )?;
        let constraint = crate::locals::resolved_replacement(constraint);
        if !crate::equality::abstract_equal(&constraint, &determining)? {
            let satisfied = call_named_complete(
                &constraint,
                plan.satisfied,
                plan.satisfied_pc,
                &[],
            )?;
            if crate::vm::is_truthy(&satisfied) {
                let _ = call_named_complete(
                    &collection,
                    plan.add,
                    plan.add_pc,
                    std::slice::from_ref(&constraint),
                )?;
            }
        }
        crate::execution_trace::kernel("constraint_collection_loop", false);
        index += 1.0;
    }
    Ok(Some(crate::value::Value::Undefined))
}

#[derive(Clone, Copy)]
struct ConstraintCollectionLoop<'a> {
    main: crate::machine::CodeView<'a>,
    test: crate::machine::CodeView<'a>,
    body: crate::machine::CodeView<'a>,
    satisfied: crate::machine::CodeView<'a>,
    add: crate::machine::CodeView<'a>,
    determining_pc: usize,
    constraints_pc: usize,
    size_pc: usize,
    constraint_pc: usize,
    satisfied_pc: usize,
    add_pc: usize,
}

fn get_named_complete(
    receiver: &crate::value::Value,
    code: crate::machine::CodeView<'_>,
    pc: usize,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let metadata = code.metadata_at(pc).ok_or(crate::execute::VmError::MissingReturn)?;
    let key = metadata.name.as_deref().ok_or(crate::execute::VmError::MissingReturn)?;
    crate::vm::get_named_property_result(receiver, key, &metadata.named_cache)
}

fn match_constraint_collection_loop(
    function: &std::rc::Rc<crate::value::FunctionValue>,
) -> Option<ConstraintCollectionLoop<'_>> {
    let main = function.code.code()?;
    if main.len() != 10 {
        return None;
    }
    let loop_instruction = main.instruction(7)?;
    let crate::ops::Op::Loop { init, test, body, update, post_test, .. } = main.cold(loop_instruction)? else {
        return None;
    };
    let (init, test, body, update) = (init.code()?, test.code()?, body.code()?, update.code()?);
    let conditional_instruction = body.instruction(8)?;
    let crate::ops::Op::Conditional { consequent, .. } = body.cold(conditional_instruction)? else {
        return None;
    };
    let satisfied = consequent.code()?;
    let branch_instruction = body.instruction(10)?;
    let crate::ops::Op::Branch { then_ops, .. } = body.cold(branch_instruction)? else {
        return None;
    };
    let add = then_ops.code()?;
    constraint_collection_shape(main, init, test, body, update, satisfied, add, *post_test)
        .then_some(ConstraintCollectionLoop {
            main, test, body, satisfied, add,
            determining_pc: 1, constraints_pc: 4, size_pc: 2,
            constraint_pc: 1, satisfied_pc: 1, add_pc: 1,
        })
}

fn constraint_collection_shape(
    main: crate::machine::CodeView<'_>, init: crate::machine::CodeView<'_>,
    test: crate::machine::CodeView<'_>, body: crate::machine::CodeView<'_>,
    update: crate::machine::CodeView<'_>, satisfied: crate::machine::CodeView<'_>,
    add: crate::machine::CodeView<'_>, post_test: bool,
) -> bool {
    use crate::ir::Opcode::*;
    !post_test
        && [1, 4].into_iter().all(|pc| main.instruction(pc).is_some_and(|op| op.opcode == GetN))
        && [2, 5].into_iter().all(|pc| main.instruction(pc).is_some_and(|op| op.opcode == InitLocal))
        && init.len() == 4
        && test.len() == 5
        && body.len() == 12
        && update.len() == 3
        && test.instruction(2).is_some_and(|op| op.opcode == CallN && op.flags == 0)
        && test.binary_at(3).is_some_and(|(_, op, _, _)| op == crate::ops::BinaryOp::LessThan)
        && body.instruction(1).is_some_and(|op| op.opcode == GetN)
        && body.instruction(3).is_some_and(|op| op.opcode == CallN && op.flags == 1)
        && body.instruction(4).is_some_and(|op| op.opcode == InitLocal)
        && body.binary_at(7).is_some_and(|(_, op, _, _)| op == crate::ops::BinaryOp::NotEqual)
        && metadata_name(main, 1) == Some("determinedBy")
        && metadata_name(main, 4) == Some("constraints")
        && metadata_name(test, 2) == Some("size")
        && metadata_name(body, 1) == Some("at")
        && metadata_name(satisfied, 1) == Some("isSatisfied")
        && metadata_name(add, 1) == Some("add")
        && update.instruction(0).is_some_and(|op| op.opcode == UpdateLocal)
}

fn metadata_name(code: crate::machine::CodeView<'_>, pc: usize) -> Option<&str> {
    code.metadata_at(pc)?.name.as_deref()
}

#[derive(Clone, Copy)]
struct PlanLoop<'a> {
    test: crate::machine::CodeView<'a>,
    body: crate::machine::CodeView<'a>,
    size_pc: usize,
    constraint_pc: usize,
    execute_pc: usize,
}

fn call_named_complete(
    receiver: &crate::value::Value,
    code: crate::machine::CodeView<'_>,
    pc: usize,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let metadata = code.metadata_at(pc).ok_or(crate::execute::VmError::MissingReturn)?;
    let key = metadata.name.as_deref().ok_or(crate::execute::VmError::MissingReturn)?;
    let callee = crate::vm::get_named_property_result(receiver, key, &metadata.named_cache)?;
    crate::functions::execute_target(&callee, receiver, arguments)
}

fn call_registered_one(
    receiver: &crate::value::Value,
    body: crate::machine::CodeView<'_>,
    pc: usize,
    index: f64,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let metadata = body.metadata_at(pc).ok_or(crate::execute::VmError::MissingReturn)?;
    let key = metadata.name.as_deref().ok_or(crate::execute::VmError::MissingReturn)?;
    let callee = crate::vm::get_named_property_result(receiver, key, &metadata.named_cache)?;
    crate::functions::execute_target(&callee, receiver, &[crate::value::Value::Number(index)])
}

fn match_plan_loop(
    function: &std::rc::Rc<crate::value::FunctionValue>,
) -> Option<PlanLoop<'_>> {
    let code = function.code.code()?;
    if function.params != 0 || code.len() != 4 {
        return None;
    }
    let loop_op = code.instruction(1)?;
    let crate::ops::Op::Loop { init, test, body, update, post_test, .. } = code.cold(loop_op)? else {
        return None;
    };
    let (init, test, body, update) = (init.code()?, test.code()?, body.code()?, update.code()?);
    plan_loop_shape(code, loop_op, init, test, body, update, *post_test).then_some(PlanLoop {
        test,
        body,
        size_pc: 2,
        constraint_pc: 1,
        execute_pc: 6,
    })
}

fn plan_loop_shape(
    code: crate::machine::CodeView<'_>,
    loop_op: crate::ir::Instruction,
    init: crate::machine::CodeView<'_>,
    test: crate::machine::CodeView<'_>,
    body: crate::machine::CodeView<'_>,
    update: crate::machine::CodeView<'_>,
    post_test: bool,
) -> bool {
    !post_test
        && loop_op.opcode == crate::ir::Opcode::Slow
        && matches!(code.constant_at(0), Some((_, crate::ops::Constant::Undefined)))
        && init.len() == 4
        && test.len() == 5
        && body.len() == 8
        && update.len() == 3
        && test.instruction(2).is_some_and(|op| op.opcode == crate::ir::Opcode::CallN && op.flags == 0)
        && test.binary_at(3).is_some_and(|(_, op, _, _)| op == crate::ops::BinaryOp::LessThan)
        && body.instruction(1).is_some_and(|op| op.opcode == crate::ir::Opcode::GetN)
        && body.instruction(3).is_some_and(|op| op.opcode == crate::ir::Opcode::CallN && op.flags == 1)
        && body.instruction(4).is_some_and(|op| op.opcode == crate::ir::Opcode::InitLocal)
        && body.instruction(6).is_some_and(|op| op.opcode == crate::ir::Opcode::CallN && op.flags == 0)
        && update.instruction(0).is_some_and(|op| op.opcode == crate::ir::Opcode::UpdateLocal)
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
    let mut owner = if matches!(receiver, crate::value::Value::Object(object) if !object.has_replacement())
    {
        match inspect_plain_property(receiver, name)? {
            Ok(value) => return Some(value),
            Err(prototype) => prototype,
        }
    } else {
        crate::locals::resolved_replacement(receiver.clone())
    };
    for _ in 1..4 {
        match inspect_plain_property(&owner, name)? {
            Ok(value) => return Some(value),
            Err(prototype) => owner = crate::locals::resolved_replacement(prototype),
        }
    }
    None
}

fn inspect_plain_property(
    owner: &crate::value::Value,
    name: &str,
) -> Option<Result<crate::value::Value, crate::value::Value>> {
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
        return crate::vm::proven_own_data(owner, name)
            .map(crate::locals::resolved_replacement)
            .map(Ok);
    }
    let prototype = match crate::vm::proven_own_data(owner, "\0prototype")? {
        crate::value::Value::ObjectAlias(alias) => crate::value::Value::Object(alias.target()?),
        prototype => prototype,
    };
    Some(Err(prototype))
}
