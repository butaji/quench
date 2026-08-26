fn execute_plan_loop(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    let Some(plan) = function.code.facts().counted_method_loop.as_deref() else {
        return Ok(None);
    };
    let mut index = 0.0;
    loop {
        let size = call_fact_named(receiver, &plan.length_method, &[])?;
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
            &plan.element_method,
            &[crate::value::Value::Number(index)],
        )?;
        let constraint = crate::locals::resolved_replacement(constraint);
        let body = crate::execute::get_property_result(&constraint, &plan.body_method)?;
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
    }
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
