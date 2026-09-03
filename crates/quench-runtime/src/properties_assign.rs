pub(crate) fn assign_set_property(
    target: &crate::value::Value,
    key: &str,
    value: crate::value::Value,
) -> Result<crate::value::Value, crate::execute::VmError> {
    reject_nullish_property_write(target)?;
    if let crate::value::Value::Proxy(_) = target {
        let result = crate::proxy::proxy_set(target, key, &value, Some(target))?;
        if matches!(result, crate::value::Value::Boolean(false)) {
            return Err(crate::value::error::throw_type_error(
                "Proxy set trap returned false",
            ));
        }
        return Ok(target.clone());
    }
    if crate::typed_array_ops::is_view(target) && crate::typed_array_ops::is_index_key(key) {
        if let Some(result) = crate::typed_array_ops::set_property(target, key, &value) {
            return result;
        }
    }
    if let crate::value::Value::Object(properties) = target {
        if crate::builtins::boxed_string_immutable_key(properties, key) {
            return Err(crate::value::error::throw_type_error(
                "Cannot assign to read-only property",
            ));
        }
    }
    let own_readonly = crate::builtins::object::descriptor(
        Some(target),
        Some(&crate::value::Value::String(key.to_string())),
    )
    .ok()
    .is_some_and(|descriptor| {
        matches!(descriptor, crate::value::Value::Object(properties) if properties.iter().any(|(name, value)| name == "writable" && matches!(value, crate::value::Value::Boolean(false))))
    });
    if own_readonly {
        return Err(crate::value::error::throw_type_error(
            "Cannot assign to read-only property",
        ));
    }
    if rejects_new_property(target, key) || inherited_write_blocked(target, key) {
        return Err(crate::value::error::throw_type_error(
            "Cannot assign to read-only property",
        ));
    }
    if let Some(setter) = crate::property_define::accessor(target, key, "set") {
        if matches!(setter, crate::value::Value::Undefined) {
            return Err(crate::value::error::throw_type_error(
                "Cannot set property without a setter",
            ));
        }
        let (_, _) = crate::functions::execute_target_with_receiver(
            &setter,
            target,
            std::slice::from_ref(&value),
        )?;
        return Ok(crate::locals::resolved_replacement(target.clone()));
    }
    Ok(crate::builtins::set_property(target.clone(), key, value))
}

pub(crate) fn assign_two_unique_plain<F>(
    target: &mut crate::value::Value,
    first: (&str, crate::value::Value),
    second_key: &str,
    second_value: F,
) -> Result<bool, crate::execute::VmError>
where
    F: FnOnce() -> Result<crate::value::Value, crate::execute::VmError>,
{
    if first.0 == second_key {
        return Ok(false);
    }
    if !unique_plain_pair_admitted(target, first.0, second_key) {
        return Ok(false);
    }
    let second_value = second_value()?;
    let crate::value::Value::Object(object) = target else {
        return Ok(false);
    };
    let Some(object) = std::rc::Rc::get_mut(object) else {
        return Ok(false);
    };
    object.set_property_in_place(first.0, first.1);
    object.set_property_in_place(second_key, second_value);
    crate::execution_trace::kernel("unique_plain_pair_write", false);
    Ok(true)
}

#[derive(Clone)]
struct UniquePlainPairPlan {
    source_layout: u32,
    prototype_layout: u32,
    prototype: std::rc::Weak<crate::value::ObjectData>,
    first: crate::identity::PropertyKeyId,
    second: crate::identity::PropertyKeyId,
    intrinsic_generation: u64,
}

thread_local! {
    static UNIQUE_PLAIN_PAIR_PLANS: std::cell::RefCell<Vec<UniquePlainPairPlan>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn unique_plain_pair_admitted(target: &crate::value::Value, first: &str, second: &str) -> bool {
    let crate::value::Value::Object(object) = target else { return false };
    if std::rc::Rc::strong_count(object) != 1
        || object.has_replacement()
    {
        return false;
    }
    if UNIQUE_PLAIN_PAIR_PLANS.with(|plans| {
        plans.borrow().iter().any(|plan| {
            unique_plain_pair_plan_matches(object, first, second, plan)
        })
    }) {
        return true;
    }
    derive_unique_plain_pair_plan(target, object, first, second)
}

fn unique_plain_pair_plan_matches(
    object: &crate::value::ObjectData,
    first: &str,
    second: &str,
    plan: &UniquePlainPairPlan,
) -> bool {
    if object.semantic_layout_id() != plan.source_layout
        || crate::identity::property_key_id(first) != plan.first
        || crate::identity::property_key_id(second) != plan.second
        || crate::builtins::intrinsic_override_generation() != plan.intrinsic_generation
    {
        return false;
    }
    let Some(prototype) = immediate_plain_prototype(object) else { return false };
    let Some(expected) = plan.prototype.upgrade() else { return false };
    std::rc::Rc::ptr_eq(&prototype, &expected)
        && prototype.semantic_layout_id() == plan.prototype_layout
}

fn derive_unique_plain_pair_plan(
    target: &crate::value::Value,
    object: &std::rc::Rc<crate::value::ObjectData>,
    first: &str,
    second: &str,
) -> bool {
    if [first, second].into_iter().any(|key| {
        object.hot_properties().position_rev(key).is_some()
            || object.has_deleted_key(key)
            || crate::builtins::boxed_string_immutable_key(object, key)
            || crate::properties::rejects_new_property(target, key)
            || crate::properties::inherited_write_blocked(target, key)
            || crate::property_define::accessor(target, key, "set").is_some()
    }) {
        return false;
    }
    let Some(prototype) = immediate_plain_prototype(object) else { return false };
    let plan = UniquePlainPairPlan {
        source_layout: object.semantic_layout_id(),
        prototype_layout: prototype.semantic_layout_id(),
        prototype: std::rc::Rc::downgrade(&prototype),
        first: crate::identity::property_key_id(first),
        second: crate::identity::property_key_id(second),
        intrinsic_generation: crate::builtins::intrinsic_override_generation(),
    };
    UNIQUE_PLAIN_PAIR_PLANS.with(|plans| {
        let mut plans = plans.borrow_mut();
        if plans.len() == 4 {
            plans.remove(0);
        }
        plans.push(plan);
    });
    true
}

fn assign_proxy_set(
    registers: &mut crate::register_file::RegisterFile,
    object: u16,
    target: &crate::value::Value,
    key: &str,
    value: crate::value::Value,
) -> Result<(), crate::execute::VmError> {
    let result = assign_set_property(target, key, value)?;
    crate::execute::write_value(registers, object, result);
    Ok(())
}

fn delete_proxy_property(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    target: &crate::value::Value,
    key: &str,
    strict: bool,
) -> Result<(), crate::execute::VmError> {
    let result = crate::proxy::proxy_delete(target, key)?;
    let deleted = matches!(result, crate::value::Value::Boolean(true));
    if !deleted && strict {
        return Err(crate::value::error::throw_type_error(
            "Cannot delete property through Proxy",
        ));
    }
    crate::execute::write_value(registers, dst, result);
    Ok(())
}
