use crate::value::ObjectData;

thread_local! {
    static FUNCTION_GLOBAL: RefCell<Option<Value>> = const { RefCell::new(None) };
}

pub(crate) fn current_global_object() -> Value {
    if let Some(global) = FUNCTION_GLOBAL.with(|slot| slot.borrow().clone()) {
        return global;
    }
    if let Some(global) = batched_global_object() {
        return Value::Object(global);
    }
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

pub(crate) fn with_function_global<T>(global: &Value, callback: impl FnOnce() -> T) -> T {
    let previous = FUNCTION_GLOBAL.with(|slot| slot.replace(Some(global.clone())));
    let result = callback();
    FUNCTION_GLOBAL.with(|slot| slot.replace(previous));
    result
}

pub(crate) fn initialize_global_object(value: &Value) {
    let Value::Object(object) = value else {
        return;
    };
    GLOBAL_OBJECT.with(|global| {
        if global.borrow().is_none() {
            global.replace(Some(object.clone()));
        }
    });
}

pub(crate) fn is_global_object(value: &Value) -> bool {
    let Value::Object(object) = value else {
        return false;
    };
    matches!(current_global_object(), Value::Object(global) if Rc::ptr_eq(&global, object))
}

pub(crate) fn is_child_global_object(value: &Value) -> bool {
    is_global_object(value) && current_context_or_default().realm() != crate::ops::RealmId::ROOT
}

pub(crate) fn begin_global_declaration_batch() {
    if GLOBAL_DECLARATION_BATCH.with(|batch| batch.borrow().is_some()) {
        return;
    }
    let Some(current) = current_global_base() else {
        return;
    };
    let staged = Rc::new(ObjectData::with_private_slots(
        current.properties.clone(),
        Rc::new(RefCell::new(current.private_slots.borrow().clone())),
    ));
    GLOBAL_DECLARATION_BASE.with(|base| base.replace(Some(current)));
    GLOBAL_DECLARATION_BATCH.with(|batch| batch.replace(Some(staged)));
}

pub(crate) fn flush_global_declaration_batch(registers: &mut Vec<Value>) {
    let Some(previous) = GLOBAL_DECLARATION_BASE.with(|batch| batch.borrow_mut().take()) else {
        return;
    };
    let Some(staged) = GLOBAL_DECLARATION_BATCH.with(|batch| batch.borrow_mut().take()) else {
        return;
    };
    synchronize_global_object(registers, &Value::Object(previous), &Value::Object(staged));
}

pub(crate) fn is_global_declaration_batch_active() -> bool {
    GLOBAL_DECLARATION_BATCH.with(|batch| batch.borrow().is_some())
}

pub(crate) fn update_global_declaration_batch(updated: &Value) {
    let Value::Object(updated) = updated else {
        return;
    };
    if !is_global_declaration_batch_active() {
        return;
    }
    GLOBAL_DECLARATION_BATCH.with(|batch| batch.replace(Some(updated.clone())));
}

fn batched_global_object() -> Option<ObjectProperties> {
    GLOBAL_DECLARATION_BATCH.with(|batch| batch.borrow().clone())
}

fn current_global_base() -> Option<ObjectProperties> {
    if let Some(global) = registered_current_global() {
        return Some(global);
    }
    GLOBAL_OBJECT.with(|global| global.borrow().clone())
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

pub(crate) fn replace_global_object(old: &Value, new: &Value) {
    let (Value::Object(old_object), Value::Object(new_object)) = (old, new) else {
        return;
    };
    if let Some(realm) = realm::id_for_global(old_object) {
        replace_realm_global(realm, new_object.clone());
    }
    GLOBAL_OBJECT.with(|global| global.replace(Some(new_object.clone())));
    crate::locals::current().replace_value(old, new);
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

thread_local! {
    static GLOBAL_DECLARATION_BASE: RefCell<Option<ObjectProperties>> = const { RefCell::new(None) };
    static GLOBAL_DECLARATION_BATCH: RefCell<Option<ObjectProperties>> =
        const { RefCell::new(None) };
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
