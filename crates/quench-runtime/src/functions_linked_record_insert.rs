fn execute_linked_record_insert(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Option<Result<crate::value::Value, crate::execute::VmError>> {
    let fact = function.code.facts().linked_record_insert.as_deref()?;
    let crate::value::Value::Object(receiver) = receiver else {
        return None;
    };
    if receiver.has_replacement() || arguments.len() < 4 {
        return None;
    }
    let current = writable_linked_record_word(receiver, &fact.current)?;
    let list = writable_linked_record_word(receiver, &fact.list)?;
    let index = writable_linked_record_word(receiver, &fact.index)?;

    let constructor = function.captures.get(fact.constructor_slot);
    let constructor_arguments = [
        list.load(),
        arguments[0].clone(),
        arguments[1].clone(),
        arguments[2].clone(),
        arguments[3].clone(),
    ];
    let record = match crate::construct::construct_value(&constructor, &constructor_arguments) {
        Ok(record) => record,
        Err(error) => return Some(Err(error)),
    };

    current.store(record.clone());
    list.store(record.clone());
    let key = match crate::conversion::to_property_key(&arguments[0]) {
        Ok(key) => key,
        Err(error) => return Some(Err(error)),
    };
    let indexed = match crate::properties::assign_set_property(&index.load(), &key, record) {
        Ok(indexed) => indexed,
        Err(error) => return Some(Err(error)),
    };
    index.store(indexed);
    crate::execution_trace::kernel("linked_record_insert", false);
    Some(Ok(crate::value::Value::Undefined))
}

fn writable_linked_record_word<'a>(
    object: &'a crate::value::ObjectData,
    key: &str,
) -> Option<&'a crate::register_file::SlotWord> {
    if object.hot_properties().names().any(|name| {
        crate::builtins::is_deleted_key_for(name, key)
            || crate::builtins::is_descriptor_key_for(name, key)
    }) {
        return None;
    }
    crate::vm::proven_own_word(object, key)
}
