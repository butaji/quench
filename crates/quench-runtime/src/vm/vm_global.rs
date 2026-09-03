use std::cell::Cell;

pub fn current_global_object() -> Value {
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
        .unwrap_or_else(|| crate::locals::current().get(0))
}

pub(crate) const SCRIPT_GLOBAL_VIEW: &str = "\0quench:script-global-view";

thread_local! {
    static RESOLVING_GLOBAL_OWNER: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn initialize_global_object(value: &Value) {
    let Value::Object(object) = value else {
        return;
    };
    object.mark_realm_global();
    GLOBAL_OBJECT.with(|global| {
        if global.borrow().is_none() {
            global.replace(Some(object.clone()));
        }
    });
}

pub(crate) fn is_global_object(value: &Value) -> bool {
    let Some(object) = global_object_target(value) else {
        return false;
    };
    if GLOBAL_DECLARATION_BASE.with(|base| {
        base.borrow()
            .as_ref()
            .is_some_and(|base| Rc::ptr_eq(base, &object))
    }) {
        return true;
    }
    matches!(current_global_object(), Value::Object(global) if global.identity() == object.identity())
}

/// Return the live object owned by the realm for a global receiver retained
/// across declaration staging or copy-on-write replacement.
pub(crate) fn resolve_global_owner(value: &Value) -> Option<Value> {
    let object = global_object_target(value)?;
    // A realm global alias carries its semantic owner in the realm registry;
    // do not infer ownership through the alias's `globalThis` property, which
    // can resolve through a stale COW replacement while the alias is being
    // retargeted.
    if let Some(realm) = realm::id_for_global(&object) {
        return realm::global(realm);
    }
    if matches!(value, Value::Object(view) if view.iter().any(|(name, _)| name == SCRIPT_GLOBAL_VIEW))
        || matches!(value, Value::ObjectAlias(alias) if alias
            .target()
            .is_some_and(|view| view.iter().any(|(name, _)| name == SCRIPT_GLOBAL_VIEW)))
    {
        return Some(current_global_object());
    }
    if let Some(staged) = batched_global_object() {
        let base = GLOBAL_DECLARATION_BASE.with(|base| base.borrow().clone());
        if base.is_some_and(|base| Rc::ptr_eq(&base, &object)) {
            return Some(Value::Object(staged));
        }
    }
    // A deferred module namespace is an ordinary object for ownership
    // purposes, but probing its `globalThis` marker would invoke its evaluator
    // and violate the namespace's symbol-like-key deferral rules.
    if crate::module_bindings::is_namespace(value) {
        return None;
    }
    // Global aliases expose a self-referential `globalThis` property.  That
    // owner marker survives copy-on-write storage replacement even when the
    // alias itself still points at the previous object allocation.
    let global_this = if matches!(value, Value::ObjectAlias(_)) {
        RESOLVING_GLOBAL_OWNER.with(|active| {
            if active.replace(true) {
                return None;
            }
            let result = crate::execute::get_property_result(value, "globalThis");
            active.set(false);
            result.ok()
        })
    } else {
        None
    };
    if global_this
        .and_then(|global_this| global_object_target(&global_this))
        .is_some_and(|global_this| global_this.identity() == object.identity())
    {
        return Some(live_global_for_target(&object));
    }
    is_global_object(value).then(|| live_global_for_target(&object))
}

fn global_object_target(value: &Value) -> Option<ObjectProperties> {
    match value {
        Value::Object(object) => Some(object.clone()),
        Value::ObjectAlias(alias) => alias.target(),
        _ => None,
    }
}

fn live_global_for_target(object: &ObjectProperties) -> Value {
    realm::id_for_global(object)
        .and_then(realm::global)
        .unwrap_or_else(current_global_object)
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
    let mut properties = current.properties.clone();
    if let Some(math) = realm::intrinsic(current_realm(), crate::ops::Builtin::Math) {
        if let Some(slot) = properties.position_rev("Math") {
            let mut value = properties.slot_value(slot).unwrap_or(Value::Undefined);
            match &mut value {
                Value::BindingCell(cell) => cell.store(math),
                value => *value = Value::BindingCell(crate::value::BindingCell::new(math)),
            }
            properties.store_slot(slot, value);
        } else {
            properties.push((
                "Math".into(),
                Value::BindingCell(crate::value::BindingCell::new(math)),
            ));
        }
    }
    let staged = Rc::new(crate::value::ObjectData::with_shared_properties_for_owner(
        &current,
        properties,
        Rc::new(RefCell::new(current.private_slots.borrow().clone())),
    ));
    GLOBAL_DECLARATION_BASE.with(|base| base.replace(Some(current)));
    GLOBAL_DECLARATION_BATCH.with(|batch| batch.replace(Some(staged)));
    GLOBAL_DECLARATION_ACTIVE.with(|active| active.set(true));
}

pub(crate) fn flush_global_declaration_batch(registers: &mut crate::register_file::RegisterFile) {
    if !GLOBAL_DECLARATION_ACTIVE.with(|active| active.get()) {
        return;
    }
    GLOBAL_DECLARATION_ACTIVE.with(|active| active.set(false));
    let Some(previous) = GLOBAL_DECLARATION_BASE.with(|batch| batch.borrow_mut().take()) else {
        return;
    };
    let Some(staged) = GLOBAL_DECLARATION_BATCH.with(|batch| batch.borrow_mut().take()) else {
        return;
    };
    synchronize_global_object(registers, &Value::Object(previous), &Value::Object(staged));
}

pub(crate) fn is_global_declaration_batch_active() -> bool {
    GLOBAL_DECLARATION_ACTIVE.with(|active| active.get())
}

pub(crate) fn define_global_declaration_property(
    name: &str,
    descriptor: &[(String, crate::value::Value)],
) -> bool {
    GLOBAL_DECLARATION_BATCH.with(|batch| {
        let mut batch = batch.borrow_mut();
        let Some(staged) = batch.as_mut() else {
            return false;
        };
        let Some(staged) = std::rc::Rc::get_mut(staged) else {
            return false;
        };
        let descriptor_key = crate::builtins::descriptor_key(name);
        let deleted_key = crate::builtins::deleted_key(name);
        // A global function declaration may redefine an existing
        // non-configurable property with a value-only descriptor.  The
        // ordinary DefineProperty path preserves that property's flags;
        // complete the staged descriptor before replacing its metadata.
        let previous_descriptor = staged.properties.iter().rev().find_map(|(key, value)| {
            (key == &descriptor_key).then_some(value)
        });
        let mut effective_descriptor = descriptor.to_vec();
        if let Some(crate::value::Value::Object(previous)) = previous_descriptor {
            for field in ["writable", "enumerable", "configurable", "get", "set"] {
                if effective_descriptor.iter().any(|(key, _)| key == field) {
                    continue;
                }
                if let Some(value) = previous
                    .iter()
                    .rev()
                    .find_map(|(key, value)| (key == field).then_some(value.clone()))
                {
                    effective_descriptor.push((field.to_string(), value));
                }
            }
        }
        staged.ensure_creation_order();
        staged.properties.retain(|(key, _)| {
            key != name && key != &descriptor_key && key != &deleted_key
        });
        let value = effective_descriptor
            .iter()
            .rev()
            .find_map(|(key, value)| (key == "value").then(|| value.clone()))
            .unwrap_or(crate::value::Value::Undefined);
        staged.properties.push((name.into(), value));
        staged.properties.push((
            descriptor_key.into(),
            crate::value::Value::Object(std::rc::Rc::new(
                crate::value::ObjectData::new(effective_descriptor),
            )),
        ));
        if !staged.created.iter().any(|key| key == name) {
            staged.created.push(name.into());
        }
        staged.invalidate_layout();
        true
    })
}

pub(crate) fn update_global_declaration_batch(updated: &Value) {
    let Value::Object(updated) = updated else {
        return;
    };
    if !is_global_declaration_batch_active() {
        return;
    }
    let owner = GLOBAL_DECLARATION_BASE.with(|base| base.borrow().clone());
    let staged = owner
        .as_ref()
        .map(|owner| {
            Rc::new(crate::value::ObjectData::with_shared_properties_for_owner(
                owner,
                updated.properties.clone(),
                Rc::new(RefCell::new(updated.private_slots.borrow().clone())),
            ))
        })
        .unwrap_or_else(|| updated.clone());
    GLOBAL_DECLARATION_BATCH.with(|batch| batch.replace(Some(staged)));
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

pub(crate) fn synchronize_global_object(
    registers: &mut crate::register_file::RegisterFile,
    old: &Value,
    new: &Value,
) {
    let old_object = match old {
        Value::Object(object) => std::rc::Rc::clone(object),
        Value::ObjectAlias(alias) => match alias.target() {
            Some(object) => object,
            None => return,
        },
        _ => return,
    };
    let Value::Object(new_object) = new else {
        return;
    };
    if let Some(Value::Object(owner)) = resolve_global_owner(old) {
        if !std::rc::Rc::ptr_eq(&owner, &old_object) {
            if is_global_declaration_batch_active() {
                update_global_declaration_batch(new);
            } else {
                replace_global_object(&Value::Object(owner.clone()), new);
            }
            crate::locals::replace_value(old, new);
            retarget_global_alias(old, new_object);
            replace_register_aliases(registers, &owner, new_object);
            return;
        }
    }
    // Script `this` aliases resolve to the staged global while declaration
    // instantiation is active.  A copy-on-write property write must update
    // that batch; waiting for the final flush would otherwise discard the
    // replacement and leave the real global view stale.
    if batched_global_object().is_some_and(|staged| Rc::ptr_eq(&staged, &old_object)) {
        update_global_declaration_batch(new);
        crate::locals::replace_value(old, new);
        retarget_global_alias(old, new_object);
        replace_register_aliases(registers, &old_object, new_object);
        return;
    }
    let realm = realm::id_for_global(&old_object);
    let singleton = singleton_matches(&old_object);
    if realm.is_none() && !singleton {
        return;
    }
    if let Some(realm) = realm {
        replace_realm_global(realm, new_object.clone());
    }
    if singleton {
        GLOBAL_OBJECT.with(|global| global.replace(Some(new_object.clone())));
    }
    crate::locals::replace_value(old, new);
    retarget_global_alias(old, new_object);
    replace_register_aliases(registers, &old_object, new_object);
}

fn retarget_global_alias(old: &Value, new: &ObjectProperties) {
    if let Value::ObjectAlias(alias) = old {
        alias.retarget(std::rc::Rc::downgrade(new));
    }
}

pub(crate) fn replace_global_object(old: &Value, new: &Value) {
    let (Value::Object(old_object), Value::Object(new_object)) = (old, new) else {
        return;
    };
    if let Some(realm) = realm::id_for_global(old_object) {
        replace_realm_global(realm, new_object.clone());
    }
    GLOBAL_OBJECT.with(|global| global.replace(Some(new_object.clone())));
    crate::locals::replace_value(old, new);
}

fn replace_register_aliases(
    registers: &mut crate::register_file::RegisterFile,
    old: &ObjectProperties,
    new: &ObjectProperties,
) {
    for index in 0..registers.len() {
        let replace = matches!(registers.read(index), Some(Value::Object(object)) if Rc::ptr_eq(&object, old));
        if replace {
            if let Ok(index) = u16::try_from(index) {
                write_value(registers, index, Value::Object(new.clone()));
            }
        }
    }
}

fn current_realm() -> RealmId {
    // Nested calls into a child realm install that realm's context while the
    // caller's global storage is still live. Prefer the execution context so
    // global name resolution and COW ownership observe the callee's realm.
    let context_realm = CURRENT_CONTEXT.with(|context| {
        context
            .borrow()
            .as_ref()
            .map(|rc| rc.realm())
    });
    if let Some(realm) = context_realm.filter(|realm| *realm != RealmId::ROOT) {
        return realm;
    }
    // During declaration staging the VM context can still describe the
    // caller while GLOBAL_OBJECT is already the active realm's object.
    // Resolve the realm from that object first so intrinsic seeding uses the
    // same realm as the global binding cell.
    if let Some(realm) =
        GLOBAL_OBJECT.with(|global| global.borrow().as_ref().and_then(realm::id_for_global))
    {
        return realm;
    }
    CURRENT_CONTEXT.with(|context| {
        context
            .borrow()
            .as_ref()
            .map_or(RealmId::ROOT, |rc| rc.realm())
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

pub(crate) fn current_realm_intrinsic(builtin: crate::ops::Builtin) -> Option<Value> {
    realm::intrinsic(current_realm(), builtin)
}

pub(crate) fn replace_realm_global_if_current(
    previous: &ObjectProperties,
    next: &ObjectProperties,
) {
    if let Some(realm) = realm::id_for_global(previous) {
        replace_realm_global(realm, next.clone());
    }
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
    static GLOBAL_DECLARATION_ACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static SHARED_GLOBAL: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Keep one global object alive across nested residual executions.
pub struct SharedGlobal {
    previous: Option<ObjectProperties>,
}

/// Drop the thread-local root global between independent top-level programs.
/// Nested executions retain the caller's object through `SharedGlobal`; only a
/// top-level runner may clear this slot to give each independent program a fresh
/// global environment.
pub fn reset_global_object() {
    if SHARED_GLOBAL.with(|count| count.get() != 0) {
        return;
    }
    GLOBAL_OBJECT.with(|global| global.take());
    GLOBAL_DECLARATION_BASE.with(|base| base.take());
    GLOBAL_DECLARATION_BATCH.with(|batch| batch.take());
    GLOBAL_DECLARATION_ACTIVE.with(|active| active.set(false));
}

impl SharedGlobal {
    pub fn install() -> Self {
        let previous = GLOBAL_OBJECT.with(|global| global.borrow().clone());
        if previous.is_none() {
            let created = std::rc::Rc::new(crate::value::ObjectData::new(Vec::new()));
            initialize_global_object(&Value::Object(created));
        }
        SHARED_GLOBAL.with(|count| count.set(count.get().saturating_add(1)));
        Self { previous }
    }
}

impl Drop for SharedGlobal {
    fn drop(&mut self) {
        let remaining = SHARED_GLOBAL.with(|count| {
            let next = count.get().saturating_sub(1);
            count.set(next);
            next
        });
        if remaining == 0 {
            GLOBAL_OBJECT.with(|global| global.replace(self.previous.take()));
        }
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
        if SHARED_GLOBAL.with(|count| count.get() > 0) {
            return;
        }
        if let Some(realm) = self.realm {
            let current = GLOBAL_OBJECT.with(|slot| slot.replace(self.previous.take()));
            if let Some(current) = current {
                let current = if current.iter().any(|(name, _)| name == SCRIPT_GLOBAL_VIEW) {
                    match resolve_global_owner(&Value::Object(current.clone())) {
                        Some(Value::Object(owner)) => owner,
                        _ => current,
                    }
                } else {
                    current
                };
                replace_realm_global(realm, current);
            }
        } else if self.restore {
            GLOBAL_OBJECT.with(|global| global.replace(self.previous.take()));
        }
    }
}
