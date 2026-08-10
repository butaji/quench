pub(crate) fn current_global_object() -> Value {
    if let Some(global) = registered_current_global() {
        return Value::Object(global);
    }
    GLOBAL_OBJECT
        .with(|global| {
            global
                .borrow()
                .as_ref()
                .map(|object| Value::Object(object.clone()))
        })
        .unwrap_or(Value::Undefined)
}

pub(crate) fn is_global_object(value: &Value) -> bool {
    let Value::Object(object) = value else {
        return false;
    };
    matches!(current_global_object(), Value::Object(global) if Rc::ptr_eq(&global, object))
}

pub(crate) fn synchronize_global_object(registers: &mut Vec<Value>, old: &Value, new: &Value) {
    let (Value::Object(old_object), Value::Object(new_object)) = (old, new) else {
        return;
    };
    let realm = realm::id_for_global(old_object);
    let singleton = singleton_matches(old_object);
    if realm.is_none() && !singleton {
        return;
    }
    if let Some(realm) = realm {
        replace_realm_global(realm, new_object.clone());
    }
    if singleton {
        GLOBAL_OBJECT.with(|global| global.replace(Some(new_object.clone())));
    }
    crate::locals::current().replace_value(old, new);
    replace_register_aliases(registers, old_object, new_object);
}

fn replace_register_aliases(
    registers: &mut Vec<Value>,
    old: &ObjectProperties,
    new: &ObjectProperties,
) {
    for index in 0..registers.len() {
        let replace = matches!(&registers[index], Value::Object(object) if Rc::ptr_eq(object, old));
        if replace {
            if let Ok(index) = u16::try_from(index) {
                write_value(registers, index, Value::Object(new.clone()));
            }
        }
    }
}

fn current_realm() -> RealmId {
    CURRENT_CONTEXT.with(|context| {
        context
            .borrow()
            .as_ref()
            .map_or(RealmId::ROOT, VmContext::realm)
    })
}

fn registered_current_global() -> Option<ObjectProperties> {
    let Value::Object(global) = realm::global(current_realm())? else {
        return None;
    };
    Some(global)
}

fn singleton_matches(object: &ObjectProperties) -> bool {
    GLOBAL_OBJECT.with(|global| {
        global
            .borrow()
            .as_ref()
            .is_some_and(|candidate| Rc::ptr_eq(candidate, object))
    })
}

fn replace_realm_global(realm: RealmId, global: ObjectProperties) {
    if let Some(token) = realm::token(realm) {
        realm::register_global(&token, global);
    }
}

impl GlobalObjectGuard {
    fn install() -> Self {
        if let Some(global) = registered_current_global() {
            let previous = GLOBAL_OBJECT.with(|slot| slot.replace(Some(global)));
            return Self {
                previous,
                restore: true,
                realm: Some(current_realm()),
            };
        }
        let previous = GLOBAL_OBJECT.with(|global| global.borrow().clone());
        Self {
            restore: previous.is_none(),
            previous,
            realm: None,
        }
    }
}

impl Drop for GlobalObjectGuard {
    fn drop(&mut self) {
        if let Some(realm) = self.realm {
            let current = GLOBAL_OBJECT.with(|slot| slot.replace(self.previous.take()));
            if let Some(current) = current {
                replace_realm_global(realm, current);
            }
        } else if self.restore {
            GLOBAL_OBJECT.with(|global| global.replace(self.previous.take()));
        }
    }
}
