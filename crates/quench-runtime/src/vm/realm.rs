use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use crate::{
    ops::{Builtin, HostCapabilityKind, HostCapabilityRef, RealmId},
    value::{HostCapabilityValue, ObjectAliasValue, Value, WeakObject},
};

use super::{ObjectProperties, VmContext, VmError};

struct RealmState {
    id: RealmId,
    global: RefCell<ObjectProperties>,
    context: VmContext,
    token: Rc<HostCapabilityValue>,
    intrinsics: RefCell<Vec<(Builtin, Value)>>,
    intl_fallback_symbol: RefCell<Option<Value>>,
    global_aliases: RefCell<Vec<Weak<RefCell<WeakObject>>>>,
}

struct ExecutionGuard {
    previous: Option<ObjectProperties>,
}

thread_local! {
    static REALMS: RefCell<Vec<Option<Rc<RealmState>>>> = const { RefCell::new(Vec::new()) };
    static ROOT_INTRINSICS: RefCell<Vec<(Builtin, Value)>> = const { RefCell::new(Vec::new()) };
    static ROOT_INTL_FALLBACK_SYMBOL: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NEXT_REALM: Cell<u64> = const { Cell::new(1) };
}

/// Drop child realms after an independent fixture has finished.
pub(super) fn reset_fixture_state() {
    REALMS.with(|realms| realms.replace(Vec::new()));
    NEXT_REALM.with(|next| next.set(1));
}

pub(super) fn create(parent: &VmContext) -> RealmId {
    create_with_host_filter(parent, &[])
}

pub(super) fn create_without_host_values(parent: &VmContext, hidden: &[&str]) -> RealmId {
    create_with_host_filter(parent, hidden)
}

fn create_with_host_filter(parent: &VmContext, hidden: &[&str]) -> RealmId {
    let id = NEXT_REALM.with(|next| {
        let id = next.get();
        next.set(id.saturating_add(1));
        RealmId::new(id)
    });
    let state = Rc::new(RealmState {
        id,
        global: RefCell::new(Rc::new(crate::value::ObjectData::new(Vec::new()))),
        context: child_context(parent, id, hidden),
        token: Rc::new(HostCapabilityValue::new(HostCapabilityRef {
            realm: id,
            kind: HostCapabilityKind::GetGlobal,
        })),
        intrinsics: RefCell::new(Vec::new()),
        intl_fallback_symbol: RefCell::new(None),
        global_aliases: RefCell::new(Vec::new()),
    });
    register(state);
    id
}

pub(super) fn register_global(token: &HostCapabilityValue, global: ObjectProperties) -> bool {
    let Some(state) = state(token.realm()) else {
        return false;
    };
    if !state.token.same_identity(token) {
        return false;
    }
    state.global.replace(global);
    let target = Rc::downgrade(&state.global.borrow().clone());
    state.global_aliases.borrow_mut().retain(|alias| {
        let Some(alias) = alias.upgrade() else {
            return false;
        };
        *alias.borrow_mut() = target.clone();
        true
    });
    true
}

pub(super) fn register_global_alias(realm: RealmId, alias: &ObjectAliasValue) {
    if let Some(state) = state(realm) {
        *alias.0.borrow_mut() = Rc::downgrade(&state.global.borrow().clone());
        state
            .global_aliases
            .borrow_mut()
            .push(Rc::downgrade(&alias.0));
    }
}

pub(super) fn global(id: RealmId) -> Option<Value> {
    state(id).map(|state| Value::Object(state.global.borrow().clone()))
}

pub(super) fn global_identity(id: RealmId) -> Option<u64> {
    state(id).map(|state| state.global.borrow().identity())
}

pub(super) fn initialize_current_global(global: ObjectProperties) {
    let initialized = super::GLOBAL_OBJECT.with(|slot| {
        if slot.borrow().is_some() {
            return false;
        }
        slot.replace(Some(global.clone()));
        true
    });
    if !initialized {
        return;
    }
    let realm = super::CURRENT_CONTEXT.with(|context| {
        context
            .borrow()
            .as_ref()
            .map_or(RealmId::ROOT, |rc| rc.realm())
    });
    if let Some(state) = state(realm) {
        state.global.replace(global);
    }
}

pub(super) fn context(id: RealmId) -> Option<VmContext> {
    state(id).map(|state| state.context.clone())
}

pub(super) fn token(id: RealmId) -> Option<Rc<HostCapabilityValue>> {
    state(id).map(|state| Rc::clone(&state.token))
}

pub(super) fn id_for_token(token: &HostCapabilityValue) -> Option<RealmId> {
    let state = state(token.realm())?;
    state.token.same_identity(token).then_some(state.id)
}

pub(super) fn execute(id: RealmId, code: crate::machine::CodeView<'_>) -> Result<Value, VmError> {
    let state = state(id).ok_or_else(missing_realm)?;
    let context = state.context.clone();
    let global = Value::Object(state.global.borrow().clone());
    let caller = crate::locals::current();
    let environment = crate::environment::Environment::new();
    environment.set(0, global.clone());
    let mut registers = crate::register_file::RegisterFile::new();
    let result = {
        let _context = super::ContextGuard::install(&context);
        let _realm = ExecutionGuard::install(Rc::clone(&state));
        let _environment = crate::locals::EnvironmentGuard::install(environment);
        let _global_lexical = crate::locals::GlobalLexicalGuard::install(crate::locals::current());
        let _with_scope = crate::with_scope::FunctionGuard::isolate();
        super::execute_code_in_place_context(code, &mut registers, &context)
    };
    caller.replace_value(&global, &Value::Object(state.global.borrow().clone()));
    result
}

pub(super) fn with_realm<T>(id: RealmId, callback: impl FnOnce() -> T) -> Option<T> {
    let state = state(id)?;
    let context = state.context.clone();
    let global = Value::Object(state.global.borrow().clone());
    let environment = crate::environment::Environment::new();
    environment.set(0, global);
    let _context = super::ContextGuard::install(&context);
    let _realm = ExecutionGuard::install(Rc::clone(&state));
    let _environment = crate::locals::EnvironmentGuard::install(environment);
    let _global_lexical = crate::locals::GlobalLexicalGuard::install(crate::locals::current());
    Some(callback())
}

pub(super) fn id_for_global(global: &ObjectProperties) -> Option<RealmId> {
    REALMS.with(|realms| {
        realms.borrow().iter().flatten().find_map(|state| {
            (state.global.borrow().identity() == global.identity()).then_some(state.id)
        })
    })
}

pub(super) fn intrinsic(id: RealmId, builtin: Builtin) -> Option<Value> {
    let state = state(id);
    if state.is_none() && id == RealmId::ROOT && builtin == Builtin::AbstractModuleSource {
        return Some(root_intrinsic(builtin));
    }
    let state = state?;
    if let Some(value) = cached_intrinsic(&state, builtin) {
        return Some(value);
    }
    let value = Value::BoundFunction(Rc::new(crate::value::BoundFunctionValue {
        realm: id,
        target: Value::Builtin(builtin),
        receiver: Value::HostCapability(Rc::clone(&state.token)),
        arguments: Vec::new(),
        properties: RefCell::new(Vec::new()),
    }));
    state.intrinsics.borrow_mut().push((builtin, value.clone()));
    Some(value)
}

pub(super) fn intl_fallback_symbol(id: RealmId) -> Option<Value> {
    if id == RealmId::ROOT {
        return ROOT_INTL_FALLBACK_SYMBOL.with(|slot| {
            if let Some(symbol) = slot.borrow().clone() {
                return Some(symbol);
            }
            let symbol = crate::intl::tolocale::symbol::legacy_symbol().ok()?;
            *slot.borrow_mut() = Some(symbol.clone());
            Some(symbol)
        });
    }
    let state = state(id)?;
    if let Some(symbol) = state.intl_fallback_symbol.borrow().clone() {
        return Some(symbol);
    }
    let symbol = crate::intl::tolocale::symbol::legacy_symbol().ok()?;
    *state.intl_fallback_symbol.borrow_mut() = Some(symbol.clone());
    Some(symbol)
}

fn root_intrinsic(builtin: Builtin) -> Value {
    ROOT_INTRINSICS.with(|intrinsics| {
        if let Some((_, value)) = intrinsics
            .borrow()
            .iter()
            .find(|(candidate, _)| *candidate == builtin)
        {
            return value.clone();
        }
        let value = Value::BoundFunction(Rc::new(crate::value::BoundFunctionValue {
            realm: RealmId::ROOT,
            target: Value::Builtin(builtin),
            receiver: Value::HostCapability(Rc::new(HostCapabilityValue::new(HostCapabilityRef {
                realm: RealmId::ROOT,
                kind: HostCapabilityKind::GetGlobal,
            }))),
            arguments: Vec::new(),
            properties: RefCell::new(Vec::new()),
        }));
        intrinsics.borrow_mut().push((builtin, value.clone()));
        value
    })
}

pub(super) fn global_builtin(key: &str) -> Option<Builtin> {
    crate::globals::builtin(key)
}

pub(super) fn global_builtin_exists(key: &str) -> bool {
    global_builtin(key).is_some() || key == "globalThis"
}

pub(super) fn is_intrinsic(bound: &crate::value::BoundFunctionValue) -> bool {
    let Value::HostCapability(token) = &bound.receiver else {
        return false;
    };
    if token.realm() == RealmId::ROOT {
        return ROOT_INTRINSICS.with(|intrinsics| {
            intrinsics.borrow().iter().any(|(_, value)| {
                matches!(value, Value::BoundFunction(value) if std::ptr::eq(value.as_ref(), bound))
            })
        });
    }
    let Some(state) = state(token.realm()) else {
        return false;
    };
    state.token.same_identity(token)
        && state.intrinsics.borrow().iter().any(|(_, value)| {
            matches!(value, Value::BoundFunction(value) if std::ptr::eq(value.as_ref(), bound))
        })
}

fn cached_intrinsic(state: &RealmState, builtin: Builtin) -> Option<Value> {
    state
        .intrinsics
        .borrow()
        .iter()
        .find_map(|(candidate, value)| (*candidate == builtin).then(|| value.clone()))
}

impl ExecutionGuard {
    fn install(state: Rc<RealmState>) -> Self {
        let global = state.global.borrow().clone();
        let previous = super::GLOBAL_OBJECT.with(|slot| slot.replace(Some(global)));
        Self { previous }
    }
}

impl Drop for ExecutionGuard {
    fn drop(&mut self) {
        super::GLOBAL_OBJECT.with(|slot| {
            slot.replace(self.previous.take());
        });
    }
}

fn missing_realm() -> VmError {
    VmError::EvalError("Realm is unavailable".to_string())
}

fn child_context(parent: &VmContext, realm: RealmId, hidden: &[&str]) -> VmContext {
    let capabilities = parent
        .capabilities
        .iter()
        .map(|capability| HostCapabilityRef {
            realm,
            kind: capability.kind,
        })
        .collect();
    let mut context = VmContext {
        output_sink: parent.output_sink.clone(),
        host: parent.host.clone(),
        realm,
        capabilities,
        host_bindings: parent
            .host_bindings
            .iter()
            .map(|(name, capability)| {
                (
                    name.clone(),
                    HostCapabilityRef {
                        realm,
                        kind: capability.kind,
                    },
                )
            })
            .collect(),
        host_values: parent.host_values.clone(),
        persistent_host_values: Vec::new(),
        can_block: parent.can_block(),
        source_text: parent.source_text.clone(),
        compiled_source_text: parent.compiled_source_text.clone(),
        source_name: parent.source_name.clone(),
        execution_budget: parent.execution_budget.clone(),
    };
    context
        .host_values
        .retain(|(name, _)| !hidden.iter().any(|candidate| *candidate == name));
    context
}

fn register(state: Rc<RealmState>) {
    let Some(index) = usize::try_from(state.id.get()).ok() else {
        return;
    };
    REALMS.with(|realms| {
        let mut realms = realms.borrow_mut();
        realms.resize_with(index.saturating_add(1), || None);
        realms[index] = Some(state);
    });
}

fn state(id: RealmId) -> Option<Rc<RealmState>> {
    let index = usize::try_from(id.get()).ok()?;
    REALMS.with(|realms| realms.borrow().get(index).and_then(Clone::clone))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_reset_drops_child_realms_and_rewinds_ids() {
        let parent = VmContext::default();
        let first = create(&parent);
        assert_eq!(first.get(), 1);
        assert!(state(first).is_some());

        reset_fixture_state();

        assert!(state(first).is_none());
        assert_eq!(create(&parent).get(), 1);
        reset_fixture_state();
    }
}
