use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    hash::{BuildHasherDefault, Hasher},
    rc::Rc,
};

use crate::{environment::Environment, execute::VmError, value::Value};

thread_local! {
    static CURRENT_ENVIRONMENT: RefCell<Option<Rc<Environment>>> = const { RefCell::new(None) };
    static GLOBAL_LEXICAL_ENVIRONMENT: RefCell<Option<Rc<Environment>>> = const { RefCell::new(None) };
    static GLOBAL_LEXICAL_REALM: RefCell<Option<crate::ops::RealmId>> = const { RefCell::new(None) };
    static REPLACEMENTS: RefCell<ReplacementMap<Replacement>> = RefCell::new(ReplacementMap::default());
    static REPLACEMENT_ROOTS: RefCell<ReplacementMap<ReplacementIdentity>> = RefCell::new(ReplacementMap::default());
    static REPLACEMENTS_ACTIVE: Cell<bool> = const { Cell::new(false) };
    static STRICT_EVAL: RefCell<bool> = const { RefCell::new(false) };
    static ACTIVE_EVAL: RefCell<bool> = const { RefCell::new(false) };
    static INITIALIZING_CLASS_NAMES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

type ReplacementMap<T> = HashMap<ReplacementIdentity, T, BuildHasherDefault<ReplacementHasher>>;

#[derive(Default)]
struct ReplacementHasher(u64);

impl Hasher for ReplacementHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }

    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = self.0.rotate_left(5) ^ u64::from(*byte);
        }
    }

    #[inline]
    fn write_u64(&mut self, value: u64) {
        self.0 = self.0.rotate_left(7) ^ value.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }

    #[inline]
    fn write_usize(&mut self, value: usize) {
        self.write_u64(value as u64);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReplacementIdentity {
    Array(usize),
    Object(u64),
    Function(usize),
}

impl std::hash::Hash for ReplacementIdentity {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        let (value, tag) = match *self {
            Self::Array(value) => (value as u64, 0),
            Self::Object(value) => (value, 1),
            Self::Function(value) => (value as u64, 2),
        };
        state.write_u64(value.wrapping_mul(3).wrapping_add(tag));
    }
}

struct Replacement {
    _owners: Vec<Value>,
    value: Value,
}

pub(crate) struct StrictEvalGuard {
    previous: bool,
    previous_active: bool,
}

impl StrictEvalGuard {
    pub(crate) fn install(active: bool) -> Self {
        let previous = STRICT_EVAL.with(|value| value.replace(active));
        let previous_active = ACTIVE_EVAL.with(|value| value.replace(true));
        Self {
            previous,
            previous_active,
        }
    }
}

impl Drop for StrictEvalGuard {
    fn drop(&mut self) {
        STRICT_EVAL.with(|value| value.replace(self.previous));
        ACTIVE_EVAL.with(|value| value.replace(self.previous_active));
    }
}

pub(crate) struct EnvironmentGuard {
    previous: Option<Rc<Environment>>,
    previous_global: Option<Rc<Environment>>,
    previous_global_realm: Option<crate::ops::RealmId>,
}

pub(crate) struct GlobalLexicalGuard {
    previous: Option<Rc<Environment>>,
}

impl GlobalLexicalGuard {
    pub(crate) fn install(environment: Rc<Environment>) -> Self {
        let previous = GLOBAL_LEXICAL_ENVIRONMENT.with(|global| global.replace(Some(environment)));
        Self { previous }
    }
}

impl Drop for GlobalLexicalGuard {
    fn drop(&mut self) {
        GLOBAL_LEXICAL_ENVIRONMENT.with(|global| global.replace(self.previous.take()));
    }
}

pub(crate) struct IterationBinding {
    environment: Rc<Environment>,
    slot: u16,
    previous: Option<Rc<RefCell<Value>>>,
}

impl IterationBinding {
    pub(crate) fn install(slot: u16, value: Value) -> Self {
        let environment = current();
        let previous = Some(environment.replace_slot(slot, value));
        Self {
            environment,
            slot,
            previous,
        }
    }
}

impl Drop for IterationBinding {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            self.environment.restore_slot(self.slot, previous);
        }
    }
}

impl EnvironmentGuard {
    pub(crate) fn install(environment: Rc<Environment>) -> Self {
        let previous = CURRENT_ENVIRONMENT.with(|current| current.replace(Some(environment)));
        // Slot zero is the global object binding.  Install it before any
        // identifier/property resolution so host values attached to the
        // running context are observed through the actual global object.
        crate::vm::initialize_global_object(
            &CURRENT_ENVIRONMENT.with(|current| current.borrow().as_ref().unwrap().get(0)),
        );
        let realm = crate::vm::current_context_or_default().realm();
        let (previous_global, previous_global_realm) = GLOBAL_LEXICAL_ENVIRONMENT.with(|global| {
            let previous_global = global.borrow().clone();
            let previous_global_realm = GLOBAL_LEXICAL_REALM.with(|value| *value.borrow());
            if previous_global.is_none() || previous_global_realm != Some(realm) {
                let current = CURRENT_ENVIRONMENT.with(|current| current.borrow().clone());
                global.replace(current);
                GLOBAL_LEXICAL_REALM.with(|value| value.replace(Some(realm)));
            }
            (previous_global, previous_global_realm)
        });
        Self {
            previous,
            previous_global,
            previous_global_realm,
        }
    }
}

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        CURRENT_ENVIRONMENT.with(|current| current.replace(self.previous.take()));
        GLOBAL_LEXICAL_ENVIRONMENT.with(|global| global.replace(self.previous_global.take()));
        GLOBAL_LEXICAL_REALM.with(|value| value.replace(self.previous_global_realm));
    }
}

pub(crate) fn current() -> Rc<Environment> {
    CURRENT_ENVIRONMENT
        .with(|current| current.borrow().clone())
        .unwrap_or_default()
}

pub(crate) fn global_lexical() -> Option<Rc<Environment>> {
    GLOBAL_LEXICAL_ENVIRONMENT.with(|global| global.borrow().clone())
}

pub(crate) fn global_has_own_name(name: &str) -> bool {
    global_lexical().is_some_and(|environment| environment.has_own_name(name))
        || crate::vm::global_builtin_exists(name)
        || matches!(crate::vm::current_global_object(), Value::Object(object) if object
            .iter()
            .any(|(key, _)| key == name))
}

pub(crate) fn global_has_lexical_name(name: &str) -> bool {
    global_lexical().is_some_and(|environment| environment.has_own_name(name))
}

pub(crate) fn has_name(name: &str) -> bool {
    current().has_name(name)
        || global_lexical().is_some_and(|environment| environment.has_name(name))
}

pub(crate) fn is_installed() -> bool {
    CURRENT_ENVIRONMENT.with(|current| current.borrow().is_some())
}

pub(crate) fn store(
    registers: &crate::register_file::RegisterFile,
    slot: u16,
    source: u16,
) -> Result<(), VmError> {
    let value = crate::execute::read_register(registers, source)?;
    if current().is_deleted(&current().slot_cell(slot)) {
        return Err(crate::value::error::throw_reference_error(&format!(
            "Cannot access deleted binding '{slot}'"
        )));
    }
    let initializing_binding = current().get(slot) == Value::Undefined;
    if current().is_immutable_slot(slot)
        && !current().is_uninitialized(slot)
        && !initializing_binding
    {
        if ACTIVE_EVAL.with(|active| *active.borrow())
            && !STRICT_EVAL.with(|strict| *strict.borrow())
        {
            return Ok(());
        }
        return Err(crate::value::error::throw_type_error(
            "Cannot assign to immutable binding",
        ));
    }
    current().set(slot, value);
    current().initialize(slot);
    if slot == 0 {
        crate::vm::initialize_global_object(&current().get(slot));
    }
    Ok(())
}

pub(crate) fn store_function_name(
    _registers: &crate::register_file::RegisterFile,
    _slot: u16,
    _source: u16,
    strict: bool,
) -> Result<(), VmError> {
    if strict {
        return Err(crate::value::error::throw_type_error(
            "Cannot assign to function name",
        ));
    }
    Ok(())
}

pub(crate) fn load_binding(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    slot: u16,
    name: &str,
    dynamic: bool,
) -> Result<(), VmError> {
    if let Some(value) = crate::with_scope::resolve_binding(name)? {
        crate::execute::write_value(registers, dst, value);
        return Ok(());
    }
    let environment = current();
    if dynamic {
        if environment.is_deleted(&environment.slot_cell(slot)) {
            return Err(crate::value::error::throw_reference_error(&format!(
                "Cannot access deleted binding '{name}'"
            )));
        }
        if environment.eval_name_aliases_slot(name, slot) {
            if let Some(value) = resolve_eval_name(name) {
                crate::execute::write_value(registers, dst, value);
                return Ok(());
            }
        }
    }
    if environment.is_uninitialized(slot) {
        return Err(crate::value::error::throw_reference_error(&format!(
            "Cannot access '{name}' before initialization"
        )));
    }
    crate::execute::write_value(registers, dst, resolved_replacement(environment.get(slot)));
    Ok(())
}

pub(crate) fn resolve_target(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    name: &str,
) -> Result<(), VmError> {
    let target = crate::with_scope::binding_target(name)?.unwrap_or(Value::Undefined);
    crate::execute::write_value(registers, dst, target);
    Ok(())
}

pub(crate) fn initialize_resolved(
    registers: &crate::register_file::RegisterFile,
    target: u16,
    slot: u16,
    name: &str,
    source: u16,
) -> Result<(), VmError> {
    let value = crate::execute::read_register(registers, source)?;
    let target = crate::execute::read_register(registers, target)?;
    current().set(slot, value.clone());
    current().initialize(slot);
    if matches!(target, Value::Undefined) {
        return Ok(());
    } else {
        crate::proxy::proxy_set(&target, name, &value, None)?;
    }
    Ok(())
}

pub(crate) fn set_resolved_local(
    registers: &crate::register_file::RegisterFile,
    target: u16,
    slot: u16,
    name: &str,
    strict: bool,
    source: u16,
) -> Result<(), VmError> {
    let mut target = crate::execute::read_register(registers, target)?;
    let value = crate::execute::read_register(registers, source)?;
    while let Some(updated) = replacement(&target) {
        target = updated;
    }
    if matches!(target, Value::Undefined) {
        ensure_initialized(slot, name)?;
        let initializing_function_binding =
            current().get(slot) == Value::Undefined && crate::conversion::is_callable(&value);
        if current().is_immutable_slot(slot) && !initializing_function_binding {
            return Err(crate::value::error::throw_type_error(
                "Cannot assign to immutable binding",
            ));
        }
        write(slot, value);
        return Ok(());
    }
    if strict && !crate::with_scope::has_property(&target, name)? {
        return Err(crate::value::error::throw_reference_error(&format!(
            "{name} is not defined"
        )));
    }
    crate::proxy::proxy_set(&target, name, &value, None)?;
    Ok(())
}

pub(crate) fn load_resolved_local(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    target: u16,
    slot: u16,
    name: &str,
) -> Result<(), VmError> {
    let target_value = crate::execute::read_register(registers, target)?;
    let value = if matches!(target_value, Value::Undefined) {
        ensure_initialized(slot, name)?;
        current().get(slot)
    } else {
        crate::execute::get_property_result(&target_value, name)?
    };
    let value = if name == "Math" && matches!(value, Value::Null | Value::Undefined) {
        // Materialized lexical globals can transiently expose a nullish Math
        // binding. Fill the receiver register with the active realm intrinsic
        // before the following SET_PROPERTY, without bypassing its setter.
        crate::vm::realm_intrinsic(crate::ops::Builtin::Math)
    } else {
        value
    };
    crate::execute::write_value(registers, dst, value);
    Ok(())
}
pub(crate) fn load(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    slot: u16,
) -> Result<(), VmError> {
    crate::execute::write_value(registers, dst, resolved_replacement(current().get(slot)));
    Ok(())
}

pub(crate) fn write(slot: u16, value: Value) {
    current().set(slot, value);
}

pub(crate) fn mark_uninitialized(slot: u16) {
    current().mark_uninitialized(slot);
}

pub(crate) fn mark_uninitialized_shared(slot: u16) {
    current().mark_uninitialized_shared(slot);
}

pub(crate) fn mark_immutable(slot: u16) {
    current().mark_immutable_slot(slot);
}

pub(crate) fn check_initialized(slot: u16, name: &str) -> Result<(), VmError> {
    ensure_initialized(slot, name)
}

pub(crate) fn initialize(slot: u16) {
    current().initialize(slot);
}

pub(crate) fn load_parameter(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    slot: u16,
) -> Result<(), VmError> {
    crate::execute::write_value(registers, dst, resolved_replacement(current().get(slot)));
    Ok(())
}

pub(crate) fn update(
    registers: &mut crate::register_file::RegisterFile,
    old_dst: u16,
    updated_dst: u16,
    slot: u16,
    decrement: bool,
) -> Result<(), VmError> {
    crate::execution_trace::event(crate::execution_trace::Event::BindingLoad);
    let delta = if decrement { -1.0 } else { 1.0 };
    let environment = current();
    if !environment.is_uninitialized(slot) {
        if let Some((old, updated)) = environment.update_number(slot, delta) {
            registers.write_number(usize::from(old_dst), old);
            registers.write_number(usize::from(updated_dst), updated);
            return Ok(());
        }
    }
    if environment.is_uninitialized(slot) {
        return ensure_initialized(slot, &format!("local_{slot}"));
    }
    crate::execute::write_value(
        registers,
        old_dst,
        resolved_replacement(environment.get(slot)),
    );
    registers.write_number(usize::from(updated_dst), 1.0);
    let operator = if decrement {
        crate::ops::BinaryOp::NumericSubtract
    } else {
        crate::ops::BinaryOp::NumericAdd
    };
    crate::vm::vm_arithmetic::execute_binary(
        registers,
        updated_dst,
        operator,
        old_dst,
        updated_dst,
    )?;
    environment.set(
        slot,
        crate::vm::read_register_unchecked(registers, updated_dst),
    );
    Ok(())
}

pub(crate) fn slot_cell(slot: u16) -> Rc<RefCell<Value>> {
    current().slot_cell(slot)
}

pub(crate) fn install_slot_cell(slot: u16, cell: Rc<RefCell<Value>>) {
    current().install_slot_cell(slot, cell);
}

fn ensure_initialized(slot: u16, name: &str) -> Result<(), VmError> {
    if current().is_uninitialized(slot) {
        return Err(crate::value::error::throw_reference_error(&format!(
            "Cannot access '{name}' before initialization"
        )));
    }
    Ok(())
}

pub(crate) fn alias_eval_name(name: &str, slot: u16) {
    let environment = current();
    if !STRICT_EVAL.with(|value| *value.borrow()) {
        environment.alias_eval_caller_name(name, slot);
    }
}

pub(crate) fn declare_global_lexical(name: &str, slot: u16, immutable: bool) {
    let binding = current().slot_cell(slot);
    let environment = global_lexical().unwrap_or_else(current);
    environment.alias_binding(name, binding);
    if immutable {
        environment.mark_immutable(name);
        current().mark_immutable_slot(slot);
    }
}

pub(crate) fn begin_class_name(name: &str) {
    INITIALIZING_CLASS_NAMES.with(|names| names.borrow_mut().push(name.to_string()));
}

pub(crate) fn end_class_name(name: &str) {
    INITIALIZING_CLASS_NAMES.with(|names| {
        let mut names = names.borrow_mut();
        if let Some(index) = names.iter().rposition(|entry| entry == name) {
            names.remove(index);
        }
    });
}

pub(crate) fn finish_class_name(name: &str) {
    end_class_name(name);
}

pub(crate) fn is_initializing_class_name(name: &str) -> bool {
    INITIALIZING_CLASS_NAMES.with(|names| names.borrow().iter().any(|entry| entry == name))
}

pub(crate) fn is_immutable_name(name: &str) -> bool {
    if is_initializing_class_name(name) {
        return false;
    }
    current().is_immutable_name(name)
        || global_lexical().is_some_and(|environment| environment.is_immutable_name(name))
}
pub(crate) fn resolve_eval_name(name: &str) -> Option<Value> {
    current().resolve_eval_name(name)
}

pub(crate) fn resolve_name(name: &str) -> Option<Value> {
    if let Some(value) = current().resolve_name(name) {
        return Some(value);
    }
    if global_has_own_name(name) {
        if let Some(value) = global_lexical().and_then(|environment| environment.resolve_name(name))
        {
            return Some(value);
        }
        return crate::execute::get_property_result(&crate::vm::current_global_object(), name).ok();
    }
    None
}
pub(crate) fn resolve_name_or_undefined(name: &str) -> Result<Value, VmError> {
    if let Some(value) = resolve_eval_name(name).or_else(|| resolve_name(name)) {
        return Ok(value);
    }
    if let Some(value) = crate::globals::immutable_value(name) {
        return Ok(value);
    }
    crate::execute::get_property_result(&crate::vm::current_global_object(), name)
}

pub(crate) fn set_named(name: &str, value: Value) -> bool {
    let written = current().set_named(name, value);
    if written {
        finish_class_name(name);
    }
    written
}

pub(crate) fn set_eval_named(name: &str, value: Value) -> bool {
    current().set_eval_named(name, value)
}

pub(crate) fn delete_named(name: &str, slot: u16) -> bool {
    let environment = current();
    environment.delete_eval_caller_name(name, slot)
}

pub(crate) fn capture(count: u16) -> Rc<Environment> {
    Environment::capture(&current(), count)
}

pub(crate) fn replace_value(old: &Value, new: &Value) {
    if replace_object(old, new) {
        return;
    }
    let Some(identity) = replacement_identity(old) else {
        return;
    };
    // Canonical interior mutation keeps identity and storage unchanged. It
    // must not publish a replacement entry or retain another owner per write.
    if replacement_identity(new) == Some(identity) {
        return;
    }
    let root = REPLACEMENT_ROOTS.with(|roots| {
        let mut roots = roots.borrow_mut();
        let root = roots.get(&identity).copied().unwrap_or(identity);
        roots.entry(identity).or_insert(root);
        if let Some(new_identity) = replacement_identity(new) {
            roots.insert(new_identity, root);
        }
        root
    });
    REPLACEMENTS.with(|replacements| {
        let mut replacements = replacements.borrow_mut();
        let replacement = replacements.entry(root).or_insert_with(|| Replacement {
            _owners: Vec::with_capacity(8),
            value: new.clone(),
        });
        if let Some(owner) = replacement_owner(old) {
            replacement._owners.push(owner);
        }
        replacement.value = new.clone();
    });
    REPLACEMENTS_ACTIVE.with(|active| active.set(true));
}

fn replace_object(old: &Value, new: &Value) -> bool {
    let Value::Object(new) = new else {
        return false;
    };
    let old = match old {
        Value::Object(old) => Some(Rc::clone(old)),
        Value::ObjectAlias(alias) => alias.target(),
        _ => None,
    };
    let Some(old) = old else {
        return false;
    };
    let old_latest = latest_object(Rc::clone(&old));
    let new_latest = latest_object(Rc::clone(new));
    if Rc::ptr_eq(&old_latest, &new_latest) {
        return true;
    }
    old_latest.replace_with(Rc::clone(&new_latest));
    if !Rc::ptr_eq(&old, &old_latest) {
        old.replace_with(new_latest);
    }
    true
}

fn latest_object(mut object: Rc<crate::value::ObjectData>) -> Rc<crate::value::ObjectData> {
    while let Some(next) = object.replacement() {
        object = next;
    }
    object
}

#[inline]
pub(crate) fn replacement(value: &Value) -> Option<Value> {
    match value {
        Value::Object(object) => return object.replacement().map(Value::Object),
        Value::ObjectAlias(alias) => {
            return alias
                .target()
                .and_then(|object| object.replacement())
                .map(Value::Object)
        }
        _ => {}
    }
    if !REPLACEMENTS_ACTIVE.with(|active| active.get()) {
        return None;
    }
    let identity = replacement_identity(value)?;
    let root = REPLACEMENT_ROOTS.with(|roots| roots.borrow().get(&identity).copied())?;
    REPLACEMENTS.with(|replacements| {
        replacements
            .borrow()
            .get(&root)
            .map(|replacement| replacement.value.clone())
    })
}

pub(crate) fn reset_replacements() {
    REPLACEMENTS.with(|replacements| replacements.borrow_mut().clear());
    REPLACEMENT_ROOTS.with(|roots| roots.borrow_mut().clear());
    REPLACEMENTS_ACTIVE.with(|active| active.set(false));
}

#[inline]
pub(crate) fn resolved_replacement(value: Value) -> Value {
    if let Some(resolved) = resolved_object_replacement(&value) {
        return resolved;
    }
    let mut value = value;
    while let Some(updated) = replacement(&value) {
        if replacement_identity(&value) == replacement_identity(&updated) {
            break;
        }
        value = updated;
    }
    value
}

fn resolved_object_replacement(value: &Value) -> Option<Value> {
    let origin = match value {
        Value::Object(object) => Rc::clone(object),
        Value::ObjectAlias(alias) => alias.target()?,
        _ => return None,
    };
    let current = latest_object(origin.replacement()?);
    if !Rc::ptr_eq(&origin, &current) {
        origin.replace_with(Rc::clone(&current));
    }
    Some(Value::Object(current))
}

#[inline]
fn replacement_identity(value: &Value) -> Option<ReplacementIdentity> {
    match value {
        Value::Array(value) => Some(ReplacementIdentity::Array(Rc::as_ptr(value) as usize)),
        Value::Object(value) => Some(ReplacementIdentity::Object(value.identity())),
        Value::ObjectAlias(value) => value
            .0
            .borrow()
            .upgrade()
            .map(|object| ReplacementIdentity::Object(object.identity())),
        Value::Function(value) => Some(ReplacementIdentity::Function(Rc::as_ptr(value) as usize)),
        _ => None,
    }
}

fn replacement_owner(value: &Value) -> Option<Value> {
    match value {
        Value::Object(object) if Rc::weak_count(object) == 0 => None,
        Value::ObjectAlias(alias) => alias.target().map(Value::Object),
        Value::Array(_) | Value::Object(_) | Value::Function(_) => Some(value.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod replacement_tests {
    use super::{
        replace_value, replacement_identity, replacement_owner, resolved_replacement, Replacement,
        REPLACEMENTS,
    };
    use crate::value::{ObjectAliasValue, ObjectData, Value};
    use std::{cell::RefCell, rc::Rc};

    #[test]
    fn object_alias_and_owner_derive_one_replacement_identity() {
        let object = Rc::new(ObjectData::new(Vec::new()));
        let owner = Value::Object(object.clone());
        let alias = Value::ObjectAlias(ObjectAliasValue(Rc::new(RefCell::new(Rc::downgrade(
            &object,
        )))));
        assert_eq!(replacement_identity(&owner), replacement_identity(&alias));
    }

    #[test]
    fn replacement_entry_retains_pointer_identity_owner() {
        let object = Rc::new(ObjectData::new(Vec::new()));
        let weak = Rc::downgrade(&object);
        let owner = Value::Object(object);
        let identity = replacement_identity(&owner).expect("object identity");
        REPLACEMENTS.with(|replacements| {
            replacements.borrow_mut().insert(
                identity,
                Replacement {
                    _owners: vec![owner],
                    value: Value::Undefined,
                },
            );
        });
        assert!(weak.upgrade().is_some());
        REPLACEMENTS.with(|replacements| replacements.borrow_mut().clear());
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn ordinary_object_without_alias_needs_no_retained_owner() {
        let owner = Value::Object(Rc::new(ObjectData::new(Vec::new())));
        assert!(replacement_owner(&owner).is_none());
    }

    #[test]
    fn ordinary_object_replacement_is_owned_by_the_object() {
        let old = Value::Object(Rc::new(ObjectData::new(Vec::new())));
        let new = Value::Object(Rc::new(ObjectData::new(vec![(
            "answer".into(),
            Value::Number(42.0),
        )])));
        replace_value(&old, &new);
        let resolved = resolved_replacement(old);
        assert!(
            matches!(resolved, Value::Object(object) if Rc::ptr_eq(&object, match &new { Value::Object(object) => object, _ => unreachable!() }))
        );
    }

    #[test]
    fn resolving_an_object_releases_intermediate_versions() {
        let old = Value::Object(Rc::new(ObjectData::new(Vec::new())));
        let middle = Value::Object(Rc::new(ObjectData::new(Vec::new())));
        let latest = Value::Object(Rc::new(ObjectData::new(Vec::new())));
        let weak_middle = match &middle {
            Value::Object(object) => Rc::downgrade(object),
            _ => unreachable!(),
        };
        replace_value(&old, &middle);
        replace_value(&middle, &latest);
        let resolved = resolved_replacement(old);
        drop(middle);
        assert!(weak_middle.upgrade().is_none());
        assert!(
            matches!(resolved, Value::Object(object) if Rc::ptr_eq(&object, match &latest { Value::Object(object) => object, _ => unreachable!() }))
        );
    }
}
