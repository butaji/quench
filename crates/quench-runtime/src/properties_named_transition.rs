const WRITE_TRANSITION_TAG: u64 = 1 << 63;
const WRITE_TRANSITION_SLOTS: usize = 4096;

#[derive(Clone)]
struct NamedWriteTransition {
    source_layout: u32,
    prototype_chain: Box<[PrototypeGuard]>,
    key: crate::identity::PropertyKeyId,
    intrinsic_generation: u64,
}

#[derive(Clone)]
struct PrototypeGuard {
    layout: u32,
    object: std::rc::Weak<crate::value::ObjectData>,
}

thread_local! {
    static NAMED_WRITE_TRANSITIONS: std::cell::RefCell<Vec<Option<NamedWriteTransition>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn named_write_source(target: &crate::value::Value, key: &str) -> Option<NamedWriteTransition> {
    let crate::value::Value::Object(object) = target else {
        return None;
    };
    if crate::vm::is_global_object(target) || object_has_key_metadata(object, key) {
        return None;
    }
    let prototype_chain = ordinary_prototype_chain(object, key)?;
    Some(NamedWriteTransition {
        source_layout: object.semantic_layout_id(),
        prototype_chain,
        key: crate::identity::property_key_id(key),
        intrinsic_generation: crate::builtins::intrinsic_override_generation(),
    })
}

fn object_has_key_metadata(object: &crate::value::ObjectData, key: &str) -> bool {
    object.hot_properties().names().any(|name| {
        name == key
            || crate::builtins::is_deleted_key_for(name, key)
            || crate::builtins::is_descriptor_key_for(name, key)
            || name == "\0quench:non_extensible"
    })
}

fn ordinary_prototype_chain(
    object: &crate::value::ObjectData,
    key: &str,
) -> Option<Box<[PrototypeGuard]>> {
    const MAX_GUARDED_PROTOTYPES: usize = 8;
    let mut chain = Vec::new();
    let mut parent = object
        .hot_properties()
        .position_rev("\0prototype")
        .and_then(|slot| object.hot_properties().slot_value(slot));
    loop {
        match parent {
            Some(crate::value::Value::Object(prototype)) => {
                if chain.len() == MAX_GUARDED_PROTOTYPES
                    || prototype.has_replacement()
                    || object_has_key_metadata(&prototype, key)
                {
                    return None;
                }
                chain.push(PrototypeGuard {
                    layout: prototype.semantic_layout_id(),
                    object: std::rc::Rc::downgrade(&prototype),
                });
                parent = prototype
                    .hot_properties()
                    .position_rev("\0prototype")
                    .and_then(|slot| prototype.hot_properties().slot_value(slot));
            }
            Some(crate::value::Value::Builtin(crate::ops::Builtin::ObjectPrototype)) | None => {
                return Some(chain.into_boxed_slice());
            }
            _ => return None,
        }
    }
}

fn try_named_write_transition(
    registers: &mut crate::register_file::RegisterFile,
    object_register: u16,
    source_register: u16,
    key: &str,
    cache: &std::cell::Cell<u64>,
) -> Result<bool, crate::execute::VmError> {
    let Some(index) = transition_index(cache.get()) else {
        return Ok(false);
    };
    let entry =
        NAMED_WRITE_TRANSITIONS.with(|entries| entries.borrow().get(index).and_then(Clone::clone));
    let Some(entry) = entry else { return Ok(false) };
    let target = crate::execute::read_register(registers, object_register)?;
    let Some(object) = guarded_transition_object(&target, key, &entry) else {
        return Ok(false);
    };
    let value = crate::execute::read_register(registers, source_register)?;
    let updated = crate::builtins::object_alias::set(object, key, value);
    crate::locals::replace_value(&target, &updated);
    crate::execute::write_value(registers, object_register, updated);
    crate::execution_trace::event(crate::execution_trace::Event::NamedSetTransitionHit);
    Ok(true)
}

fn guarded_transition_object(
    target: &crate::value::Value,
    key: &str,
    entry: &NamedWriteTransition,
) -> Option<std::rc::Rc<crate::value::ObjectData>> {
    let crate::value::Value::Object(object) = target else {
        return None;
    };
    if object.has_replacement()
        || object.semantic_layout_id() != entry.source_layout
        || crate::identity::property_key_id(key) != entry.key
        || crate::builtins::intrinsic_override_generation() != entry.intrinsic_generation
    {
        return None;
    }
    prototype_chain_matches(object, &entry.prototype_chain).then(|| std::rc::Rc::clone(object))
}

fn prototype_chain_matches(object: &crate::value::ObjectData, expected: &[PrototypeGuard]) -> bool {
    let mut parent = object
        .hot_properties()
        .position_rev("\0prototype")
        .and_then(|slot| object.hot_properties().slot_value(slot));
    for guard in expected {
        let Some(crate::value::Value::Object(prototype)) = parent else {
            return false;
        };
        let Some(guarded) = guard.object.upgrade() else {
            return false;
        };
        if prototype.has_replacement()
            || !std::rc::Rc::ptr_eq(&prototype, &guarded)
            || prototype.semantic_layout_id() != guard.layout
        {
            return false;
        }
        parent = prototype
            .hot_properties()
            .position_rev("\0prototype")
            .and_then(|slot| prototype.hot_properties().slot_value(slot));
    }
    matches!(
        parent,
        Some(crate::value::Value::Builtin(
            crate::ops::Builtin::ObjectPrototype
        )) | None
    )
}

fn install_named_write_transition(
    cache: &std::cell::Cell<u64>,
    source: Option<NamedWriteTransition>,
    updated: &crate::value::Value,
    key: &str,
) -> bool {
    let Some(source) = source else { return false };
    let crate::value::Value::Object(updated) = updated else {
        return false;
    };
    if updated.hot_properties().position_rev(key).is_none() {
        return false;
    }
    let index = transition_slot(&source);
    NAMED_WRITE_TRANSITIONS.with(|entries| {
        let mut entries = entries.borrow_mut();
        if entries.is_empty() {
            entries.resize_with(WRITE_TRANSITION_SLOTS, || None);
        }
        entries[index] = Some(source);
    });
    cache.set(WRITE_TRANSITION_TAG | index as u64 + 1);
    true
}

fn transition_slot(entry: &NamedWriteTransition) -> usize {
    let prototype = entry
        .prototype_chain
        .first()
        .map_or(std::ptr::null(), |guard| guard.object.as_ptr());
    (entry.source_layout as usize)
        .wrapping_mul(0x9e37_79b1)
        .wrapping_add(prototype as usize)
        .wrapping_add(entry.key.0 as usize)
        & (WRITE_TRANSITION_SLOTS - 1)
}

fn transition_index(cache: u64) -> Option<usize> {
    if cache & WRITE_TRANSITION_TAG == 0 {
        return None;
    }
    usize::try_from((cache & !WRITE_TRANSITION_TAG).checked_sub(1)?).ok()
}

#[cfg(test)]
mod tests {
    use super::{guarded_transition_object, named_write_source};
    use crate::value::{ObjectData, Value};
    use std::rc::Rc;

    fn receiver() -> Value {
        let prototype = Rc::new(ObjectData::new(Vec::new()));
        Value::Object(Rc::new(ObjectData::new(vec![(
            "\0prototype".into(),
            Value::Object(prototype),
        )])))
    }

    #[test]
    fn transition_fact_guards_layout_prototype_and_key() {
        let receiver = receiver();
        let fact = named_write_source(&receiver, "field").unwrap();
        assert!(guarded_transition_object(&receiver, "field", &fact).is_some());
        assert!(guarded_transition_object(&receiver, "other", &fact).is_none());
    }

    #[test]
    fn intrinsic_mutation_invalidates_transition_fact() {
        let receiver = receiver();
        let fact = named_write_source(&receiver, "field").unwrap();
        crate::builtins::write_intrinsic_override(
            crate::ops::Builtin::ObjectPrototype,
            "unrelated-transition-test",
            Value::Undefined,
        );
        assert!(guarded_transition_object(&receiver, "field", &fact).is_none());
        crate::builtins::reset_intrinsic_prototype_state();
    }

    #[test]
    fn transition_fact_guards_each_object_in_a_derived_chain() {
        let base = Rc::new(ObjectData::new(Vec::new()));
        let derived = Rc::new(ObjectData::new(vec![(
            "\0prototype".into(),
            Value::Object(Rc::clone(&base)),
        )]));
        let receiver = Value::Object(Rc::new(ObjectData::new(vec![(
            "\0prototype".into(),
            Value::Object(Rc::clone(&derived)),
        )])));
        let fact = named_write_source(&receiver, "field").unwrap();
        assert!(guarded_transition_object(&receiver, "field", &fact).is_some());

        let other_derived = Rc::new(ObjectData::new(vec![(
            "\0prototype".into(),
            Value::Object(base),
        )]));
        let other_receiver = Value::Object(Rc::new(ObjectData::new(vec![(
            "\0prototype".into(),
            Value::Object(other_derived),
        )])));
        assert!(guarded_transition_object(&other_receiver, "field", &fact).is_none());
    }
}
