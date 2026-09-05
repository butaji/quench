use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    hash::{BuildHasherDefault, Hasher},
    rc::Rc,
};

use crate::{environment::Environment, execute::VmError, value::Value};

thread_local! {
    static CURRENT_ENVIRONMENT: RefCell<Option<Rc<Environment>>> = const { RefCell::new(None) };
    // The owning Rc above is the canonical state. This derived pointer keeps
    // hot local loads from cloning/borrowing that Rc on every bytecode op.
    static CURRENT_ENVIRONMENT_PTR: Cell<*const Environment> = const { Cell::new(std::ptr::null()) };
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
    Array(u64),
    Object(u64),
    Function(usize),
}

impl std::hash::Hash for ReplacementIdentity {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        let (value, tag) = match *self {
            Self::Array(value) => (value, 0),
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
    previous_pointer: *const Environment,
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
    previous: Vec<(u16, Rc<crate::value::BindingCell>)>,
}

impl IterationBinding {
    pub(crate) fn install(slot: u16, value: Value) -> Self {
        let environment = current();
        environment.clear_immutable_slot(slot);
        let previous = vec![(slot, environment.replace_slot(slot, value))];
        Self {
            environment,
            previous,
        }
    }

    pub(crate) fn install_many<I>(bindings: I) -> Self
    where
        I: IntoIterator<Item = (u16, Value)>,
    {
        let environment = current();
        let previous = bindings
            .into_iter()
            .map(|(slot, value)| {
                environment.clear_immutable_slot(slot);
                (slot, environment.replace_slot(slot, value))
            })
            .collect();
        Self {
            environment,
            previous,
        }
    }
}

impl Drop for IterationBinding {
    fn drop(&mut self) {
        for (slot, previous) in self.previous.drain(..).rev() {
            self.environment.restore_slot(slot, previous);
        }
    }
}

impl EnvironmentGuard {
    pub(crate) fn install(environment: Rc<Environment>) -> Self {
        let previous = CURRENT_ENVIRONMENT.with(|current| current.replace(Some(environment)));
        let current_pointer = CURRENT_ENVIRONMENT.with(|current| {
            current
                .borrow()
                .as_ref()
                .map_or(std::ptr::null(), Rc::as_ptr)
        });
        let previous_pointer =
            CURRENT_ENVIRONMENT_PTR.with(|pointer| pointer.replace(current_pointer));
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
            previous_pointer,
            previous_global,
            previous_global_realm,
        }
    }

    pub(crate) fn install_eval(environment: Rc<Environment>) -> Self {
        Self::install(environment)
    }
}

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        CURRENT_ENVIRONMENT_PTR.with(|pointer| pointer.set(self.previous_pointer));
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

#[inline(always)]
pub(crate) fn with_current_ref<R>(use_environment: impl FnOnce(Option<&Environment>) -> R) -> R {
    CURRENT_ENVIRONMENT_PTR.with(|pointer| {
        // SAFETY: the pointer is derived from the owning CURRENT_ENVIRONMENT
        // Rc and is restored whenever its guard leaves scope.
        use_environment(unsafe { pointer.get().as_ref() })
    })
}

pub(crate) fn global_lexical() -> Option<Rc<Environment>> {
    GLOBAL_LEXICAL_ENVIRONMENT.with(|global| global.borrow().clone())
}

pub(crate) fn global_has_own_name(name: &str) -> bool {
    global_lexical().is_some_and(|environment| environment.has_own_name(name))
        || crate::vm::global_builtin_exists(name)
        || matches!(crate::vm::current_global_object(), Value::Object(object) if object
            .physical_slot_for_name(name)
            .is_some())
}

pub(crate) fn global_has_lexical_name(name: &str) -> bool {
    global_lexical().is_some_and(|environment| environment.has_own_name(name))
}

pub(crate) fn has_name(name: &str) -> bool {
    current().has_name(name)
        || global_lexical().is_some_and(|environment| environment.has_name(name))
}

pub(crate) fn is_installed() -> bool {
    with_current_ref(|environment| environment.is_some())
}

pub(crate) fn store(
    registers: &crate::register_file::RegisterFile,
    slot: u16,
    source: u16,
) -> Result<(), VmError> {
    let environment = current();
    if environment.is_deleted_slot(slot) {
        return Err(crate::value::error::throw_reference_error(&format!(
            "Cannot access deleted binding '{slot}'"
        )));
    }
    if environment.is_immutable_slot(slot) && !environment.is_uninitialized(slot) {
        if ACTIVE_EVAL.with(|active| *active.borrow())
            && !STRICT_EVAL.with(|strict| *strict.borrow())
        {
            return Ok(());
        }
        return Err(crate::value::error::throw_type_error(
            "Cannot assign to immutable binding",
        ));
    }
    if !environment.copy_from_register(slot, registers, source) {
        environment.set(slot, crate::execute::read_register(registers, source)?);
    }
    if slot == 0 {
        crate::vm::initialize_global_object(&environment.get(slot));
    }
    Ok(())
}

/// Store to a statically resolved, initialized local. Dynamic binding state
/// remains observable through the ordinary path; the common mutable slot
/// copies its canonical execute word without cloning an Environment or cell.
#[inline(always)]
pub(crate) fn store_proven(
    registers: &crate::register_file::RegisterFile,
    slot: u16,
    source: u16,
) -> Result<(), VmError> {
    let fast = with_current_ref(|current| {
        let environment = current?;
        if environment.is_deleted_slot(slot)
            || environment.is_immutable_slot(slot)
            || environment.is_uninitialized(slot)
        {
            return None;
        }
        Some(environment.copy_proven_from_register(slot, registers, source))
    });
    if fast == Some(true) {
        return Ok(());
    }
    store(registers, slot, source)
}

/// Environment-pinned counterpart of [`store_proven`]. The dispatch driver
/// passes the active frame fact directly when it owns the environment, so the
/// proven word copy does not need to reopen the TLS closure on each store.
#[inline(always)]
pub(crate) fn store_proven_in(
    environment: &Environment,
    registers: &crate::register_file::RegisterFile,
    slot: u16,
    source: u16,
) -> Result<(), VmError> {
    if !environment.is_deleted_slot(slot)
        && !environment.is_immutable_slot(slot)
        && !environment.is_uninitialized(slot)
        && environment.copy_proven_from_register(slot, registers, source)
    {
        return Ok(());
    }
    store(registers, slot, source)
}

/// Execute the flagged compact Move over canonical lexical words. Both slot
/// mappings remain borrowed from the one active Environment; no BindingRef or
/// semantic Value is constructed on the admitted path.
#[inline(always)]
pub(crate) fn move_proven_local(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    source: u16,
    target: u16,
) -> Result<(), VmError> {
    let fast = with_current_ref(|current| {
        let environment = current?;
        move_proven_local_in(environment, registers, dst, source, target).then_some(true)
    });
    if fast == Some(true) {
        crate::execution_trace::event(crate::execution_trace::Event::RegisterWordCopy);
        return Ok(());
    }
    load_proven(registers, dst, source)?;
    store(registers, target, dst)
}

#[inline(always)]
pub(crate) fn can_move_proven_local(environment: &Environment, source: u16, target: u16) -> bool {
    environment.has_proven_slot(source)
        && environment.has_proven_slot(target)
        && !environment.is_deleted_slot(target)
        && !environment.is_immutable_slot(target)
        && !environment.is_uninitialized(target)
}

#[inline(always)]
pub(crate) fn move_proven_local_in(
    environment: &Environment,
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    source: u16,
    target: u16,
) -> bool {
    can_move_proven_local(environment, source, target)
        && move_admitted_local_in(environment, registers, dst, source, target)
}

#[inline(always)]
pub(crate) fn move_admitted_local_in(
    environment: &Environment,
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    source: u16,
    target: u16,
) -> bool {
    environment.load_proven_into(registers, dst, source)
        && environment.copy_proven_from_register(target, registers, dst)
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
    crate::execution_trace::event(crate::execution_trace::Event::BindingLoad);
    if dynamic {
        crate::execution_trace::event(crate::execution_trace::Event::DynamicBindingLoad);
    }
    let environment = current();
    if dynamic {
        if environment.is_uninitialized(slot) {
            return Err(crate::value::error::throw_reference_error(&format!(
                "Cannot access '{name}' before initialization"
            )));
        }
        if environment.is_deleted_slot(slot) {
            return Err(crate::value::error::throw_reference_error(&format!(
                "Cannot access deleted binding '{name}'"
            )));
        }
        if let Some(value) = crate::with_scope::resolve_binding(name)? {
            crate::execute::write_value(registers, dst, value);
            return Ok(());
        }
        if environment.eval_name_aliases_slot(name, slot) {
            if let Some(value) = resolve_eval_name(name) {
                crate::execute::write_value(registers, dst, value);
                return Ok(());
            }
        }
    }
    if environment.is_deleted_slot(slot) {
        return Err(crate::value::error::throw_reference_error(&format!(
            "Cannot access deleted binding '{name}'"
        )));
    }
    if environment.is_uninitialized(slot) {
        return Err(crate::value::error::throw_reference_error(&format!(
            "Cannot access '{name}' before initialization"
        )));
    }
    if let Some(number) = environment.get_number(slot) {
        registers.write_number(usize::from(dst), number);
        return Ok(());
    }
    crate::execute::write_value(registers, dst, environment.get(slot));
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
    if matches!(target, Value::Undefined) {
        current().set(slot, value);
        current().initialize(slot);
        return Ok(());
    }
    crate::proxy::proxy_set(&target, name, &value, None)?;
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
        if current().is_immutable_slot(slot) {
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

pub(crate) fn load_resolved_binding(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    target: u16,
    name: &str,
) -> Result<(), VmError> {
    let target = crate::execute::read_register(registers, target)?;
    if matches!(target, Value::Undefined) {
        return crate::with_scope::resolve_name(registers, dst, name);
    }
    if !crate::with_scope::has_property(&target, name)? {
        return Err(crate::value::error::throw_reference_error(&format!(
            "{name} is not defined"
        )));
    }
    let value = crate::execute::get_property_result(&target, name)?;
    crate::execute::write_value(registers, dst, value);
    Ok(())
}

pub(crate) fn resolve_active_target(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    name: &str,
) -> Result<(), VmError> {
    let target = crate::with_scope::active_binding_target(name)?.unwrap_or(Value::Undefined);
    crate::execute::write_value(registers, dst, target);
    Ok(())
}
pub(crate) fn load(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    slot: u16,
) -> Result<(), VmError> {
    crate::execution_trace::event(crate::execution_trace::Event::BindingLoad);
    let environment = current();
    if environment.is_deleted_slot(slot) {
        return Err(crate::value::error::throw_reference_error(
            "Cannot access deleted binding",
        ));
    }
    environment.load_into(registers, dst, slot);
    Ok(())
}

/// Load a statically resolved, initialized local from its canonical word.
/// Captured/bridged slots are still handled by `SlotStore`; only the mapping
/// ownership and thread-local Environment clone disappear.
#[inline(always)]
pub(crate) fn load_proven(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    slot: u16,
) -> Result<(), VmError> {
    crate::execution_trace::event(crate::execution_trace::Event::BindingLoad);
    let loaded = with_current_ref(|current| {
        let Some(environment) = current else {
            return false;
        };
        if environment.is_deleted_slot(slot) {
            return false;
        }
        environment.load_proven_into(registers, dst, slot)
    });
    if !loaded {
        let environment = current();
        if environment.is_deleted_slot(slot) {
            return Err(crate::value::error::throw_reference_error(
                "Cannot access deleted binding",
            ));
        }
        environment.load_into(registers, dst, slot);
    }
    Ok(())
}

/// Load a proven local when the dispatch driver already holds the active
/// environment. Keeping that immutable fact in the dispatch state avoids a
/// TLS pointer closure on every compact `LoadLocal`; all deletion and missing
/// slot checks remain identical to `load_proven`.
#[inline(always)]
pub(crate) fn load_proven_in(
    environment: &Environment,
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    slot: u16,
) -> Result<(), VmError> {
    crate::execution_trace::event(crate::execution_trace::Event::BindingLoad);
    if environment.is_deleted_slot(slot) {
        return Err(crate::value::error::throw_reference_error(
            "Cannot access deleted binding",
        ));
    }
    if !environment.load_proven_into(registers, dst, slot) {
        environment.load_into(registers, dst, slot);
    }
    Ok(())
}

#[inline(always)]
fn load_checked_in(
    environment: &Environment,
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    slot: u16,
    name: &str,
) -> Result<(), VmError> {
    if environment.is_deleted_slot(slot) {
        return Err(crate::value::error::throw_reference_error(&format!(
            "Cannot access deleted binding '{name}'"
        )));
    }
    if environment.is_uninitialized(slot) {
        return Err(crate::value::error::throw_reference_error(&format!(
            "Cannot access '{name}' before initialization"
        )));
    }
    crate::execution_trace::event(crate::execution_trace::Event::BindingLoad);
    if !environment.load_existing_proven_into(registers, dst, slot) {
        environment.load_into(registers, dst, slot);
    }
    Ok(())
}

pub(crate) fn load_checked(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    slot: u16,
    name: &str,
) -> Result<(), VmError> {
    let fast = with_current_ref(|current| {
        current.map(|environment| load_checked_in(environment, registers, dst, slot, name))
    });
    if let Some(result) = fast {
        return result;
    }
    load_checked_in(&current(), registers, dst, slot, name)
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
    if environment.is_immutable_slot(slot) && !environment.is_uninitialized(slot) {
        return Err(crate::value::error::throw_type_error(
            "Cannot assign to immutable binding",
        ));
    }
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
    let old = environment.get(slot);
    let operator = if decrement {
        crate::ops::BinaryOp::NumericSubtract
    } else {
        crate::ops::BinaryOp::NumericAdd
    };
    let updated = crate::vm::vm_arithmetic::evaluate_binary(
        &old,
        &crate::value::Value::Number(1.0),
        operator,
    )?;
    let numeric_old = crate::vm::vm_arithmetic::evaluate_to_numeric(&old)?;
    crate::execute::write_value(registers, old_dst, numeric_old);
    crate::execute::write_value(registers, updated_dst, updated.clone());
    environment.set(slot, updated);
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
    crate::execute::write_value(registers, dst, current().get(slot));
    Ok(())
}

pub(crate) fn slot_cell(slot: u16) -> Rc<crate::value::BindingCell> {
    current().slot_cell(slot)
}

pub(crate) fn install_slot_cell(slot: u16, cell: Rc<crate::value::BindingCell>) {
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
    let deleted = environment.delete_eval_caller_name(name, slot);
    if deleted {
        environment.mark_deleted_slot(slot);
    }
    deleted
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
    if same_replacement_value(old, new) {
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
    let new = match new {
        Value::Object(new) => Rc::clone(new),
        Value::ObjectAlias(alias) => match alias.target() {
            Some(new) => new,
            None => return false,
        },
        _ => return false,
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
    let new_latest = latest_object(new);
    if Rc::ptr_eq(&old_latest, &new_latest) {
        return true;
    }
    old_latest.replace_with(Rc::clone(&new_latest));
    if !Rc::ptr_eq(&old, &old_latest) {
        old.replace_with(Rc::clone(&new_latest));
    }
    // A COW write may clone an object while preserving its semantic identity.
    // Self-referential properties are weak aliases, so retarget aliases held
    // by the new representative before the previous representative can drop.
    crate::environment::retarget_aliases_for_identity(&new_latest, old_latest.identity());
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

/// Whether an array execute word is still the current representative of its
/// identity. The common no-replacement state is one TLS boolean read; any
/// published successor returns to semantic resolution.
#[inline(always)]
pub(crate) fn array_word_is_current(array: &crate::value::ArrayData) -> bool {
    if !REPLACEMENTS_ACTIVE.with(|active| active.get()) {
        return true;
    }
    let identity = ReplacementIdentity::Array(array.identity());
    let root = REPLACEMENT_ROOTS.with(|roots| roots.borrow().get(&identity).copied());
    let Some(root) = root else { return true };
    REPLACEMENTS.with(|replacements| {
        let replacements = replacements.borrow();
        let Some(replacement) = replacements.get(&root) else {
            return true;
        };
        matches!(
            &replacement.value,
            Value::Array(current) if std::ptr::eq(current.as_ref(), array)
        )
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
        if same_replacement_value(&value, &updated) {
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
        Value::Array(value) => Some(ReplacementIdentity::Array(value.identity())),
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

fn same_replacement_value(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Array(left), Value::Array(right)) => Rc::ptr_eq(left, right),
        (Value::Object(left), Value::Object(right)) => left.identity() == right.identity(),
        (Value::Function(left), Value::Function(right)) => Rc::ptr_eq(left, right),
        _ => false,
    }
}

fn replacement_owner(value: &Value) -> Option<Value> {
    match value {
        Value::Object(object) if Rc::weak_count(object) == 0 => None,
        Value::ObjectAlias(alias) => alias.target().map(Value::Object),
        Value::Object(_) | Value::Function(_) => Some(value.clone()),
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
    fn array_replacement_keeps_identity_without_retaining_ancestors() {
        super::reset_replacements();
        let original = Rc::new(crate::value::ArrayData::new(vec![Value::Boolean(false)]));
        let stale = Value::Array(Rc::clone(&original));
        let weak = Rc::downgrade(&original);
        let mut updated = (*original).clone();
        updated.set_index(0, Value::Boolean(true));
        let latest = Value::Array(Rc::new(updated));

        replace_value(&stale, &latest);
        let resolved = resolved_replacement(stale.clone());
        assert_eq!(
            crate::execute::get_property_result(&resolved, "0").unwrap(),
            Value::Boolean(true)
        );
        assert!(replacement_owner(&stale).is_none());

        drop(stale);
        drop(original);
        assert!(weak.upgrade().is_none());
        super::reset_replacements();
    }

    #[test]
    fn array_current_word_distinguishes_latest_representative_from_stale_root() {
        super::reset_replacements();
        let original = Rc::new(crate::value::ArrayData::new(vec![Value::Number(1.0)]));
        let mut changed = original.as_ref().clone();
        changed.set_index(0, Value::Number(2.0));
        let latest = Rc::new(changed);
        replace_value(
            &Value::Array(Rc::clone(&original)),
            &Value::Array(Rc::clone(&latest)),
        );
        assert!(!super::array_word_is_current(&original));
        assert!(super::array_word_is_current(&latest));
        super::reset_replacements();
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
