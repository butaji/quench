use std::{cell::RefCell, collections::HashMap, rc::Rc};

use crate::{execute::VmError, ops::Op, value::Value};

struct Descriptor {
    value: Value,
    writable: bool,
    enumerable: bool,
    configurable: bool,
    data: bool,
}

thread_local! {
    static GLOBAL_BINDINGS: RefCell<HashMap<(crate::ops::RealmId, String), Rc<crate::value::BindingCell>>> =
        RefCell::new(HashMap::new());
}

pub(crate) fn binding_cells() -> Vec<Rc<crate::value::BindingCell>> {
    GLOBAL_BINDINGS.with(|bindings| {
        let mut seen = std::collections::HashSet::new();
        bindings
            .borrow()
            .values()
            .filter_map(|cell| {
                seen.insert(Rc::as_ptr(cell) as usize)
                    .then(|| Rc::clone(cell))
            })
            .collect()
    })
}

pub(crate) fn store_global_binding(name: &str, value: Value) -> bool {
    let realm = crate::vm::current_context_or_default().realm();
    GLOBAL_BINDINGS.with(|bindings| {
        let Some(cell) = bindings.borrow().get(&(realm, name.to_string())).cloned() else {
            return false;
        };
        cell.store(value);
        true
    })
}

pub(crate) fn execute(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
) -> Result<(), VmError> {
    match op {
        Op::CheckGlobalFunction { name } => check_function(name),
        Op::CheckGlobalVar {
            name,
            is_lexical,
            is_eval,
        } => check_var(name, *is_lexical, *is_eval),
        Op::CreateGlobalFunction {
            name,
            slot,
            deletable,
        } => create_function(registers, name, *slot, *deletable),
        Op::CreateGlobalVar {
            name,
            slot,
            deletable,
            is_lexical,
        } => create_var(registers, name, *slot, *deletable, *is_lexical),
        _ => Ok(()),
    }
}

fn check_function(name: &str) -> Result<(), VmError> {
    if crate::locals::global_has_lexical_name(name) {
        return lexical_collision(name);
    }
    if let Some(descriptor) = own_descriptor(name) {
        let compatible = descriptor.configurable
            || descriptor.data && descriptor.writable && descriptor.enumerable;
        if !compatible {
            return Err(crate::value::error::throw_type_error(&format!(
                "Cannot declare global function '{name}'"
            )));
        }
    } else if !is_global_extensible() {
        return Err(crate::value::error::throw_type_error(&format!(
            "Cannot declare global function '{name}' on non-extensible object"
        )));
    }
    Ok(())
}

fn check_var(name: &str, is_lexical: bool, is_eval: bool) -> Result<(), VmError> {
    // The reducer conflates var and lexical declarations under CheckGlobalVar.
    // Lexical declarations (let/const/class) must throw SyntaxError on existing
    // or restricted bindings, while var declarations follow CanDeclareGlobalVar:
    // if the global already has an own property of that name, the declaration
    // is permitted; otherwise the global must be extensible. Per spec
    // (GlobalDeclarationInstantiation step 6) a var declaration that collides
    // with an existing lexical declaration also throws SyntaxError.
    let _ = is_eval;
    if is_lexical {
        check_lexical_declaration(name)?;
        Ok(())
    } else if crate::locals::global_has_lexical_name(name) {
        lexical_collision(name)
    } else if own_descriptor(name).is_some() {
        Ok(())
    } else if !is_global_extensible() {
        Err(crate::value::error::throw_type_error(&format!(
            "Cannot declare global var '{name}' on non-extensible object"
        )))
    } else {
        Ok(())
    }
}

fn lexical_collision(name: &str) -> Result<(), VmError> {
    Err(VmError::Thrown(crate::builtins::error(
        crate::ops::Builtin::SyntaxError,
        &[Value::String(format!(
            "Global lexical binding '{name}' already exists"
        ))],
    )))
}

fn is_global_extensible() -> bool {
    let global = crate::vm::current_global_object();
    let value = crate::properties::is_extensible_value(Some(&global));
    matches!(value, Ok(Value::Boolean(true)))
}

fn check_lexical_declaration(name: &str) -> Result<(), VmError> {
    if !crate::locals::global_has_lexical_name(name) {
        if has_restricted_global(name) {
            return Err(VmError::Thrown(crate::builtins::error(
                crate::ops::Builtin::SyntaxError,
                &[Value::String(format!(
                    "Global lexical binding '{name}' is restricted"
                ))],
            )));
        }
        return Ok(());
    }
    Err(VmError::Thrown(crate::builtins::error(
        crate::ops::Builtin::SyntaxError,
        &[Value::String(format!(
            "Global lexical binding '{name}' already exists"
        ))],
    )))
}

fn has_restricted_global(name: &str) -> bool {
    let Some(descriptor) = own_descriptor(name) else {
        return false;
    };
    !descriptor.configurable
}

fn create_var(
    registers: &mut crate::register_file::RegisterFile,
    name: &str,
    slot: u16,
    deletable: bool,
    is_lexical: bool,
) -> Result<(), VmError> {
    // Per spec, lexical declarations (let/const/class) do not create
    // properties on the global object.
    if is_lexical {
        return Ok(());
    }
    let current = own_descriptor(name);
    if current
        .as_ref()
        .is_some_and(|descriptor| !descriptor.configurable)
    {
        return alias_existing_global(registers, name, slot, current.as_ref().unwrap());
    }
    let cell = binding_cell(name, slot, current.as_ref().map(|value| &value.value));
    // Per spec, global var bindings are non-configurable.
    let descriptor = match current {
        Some(current) if !current.configurable => value_descriptor(cell),
        Some(current) => descriptor_with_flags(cell, &current),
        None => data_descriptor(cell, true, true, deletable),
    };
    define_global(registers, name, descriptor)
}

fn alias_existing_global(
    registers: &mut crate::register_file::RegisterFile,
    name: &str,
    slot: u16,
    current: &Descriptor,
) -> Result<(), VmError> {
    let cell = binding_cell(name, slot, Some(&current.value));
    if !current.writable {
        return Ok(());
    }
    define_global(registers, name, descriptor_with_flags(cell, current))
}

fn create_function(
    registers: &mut crate::register_file::RegisterFile,
    name: &str,
    slot: u16,
    deletable: bool,
) -> Result<(), VmError> {
    let current = own_descriptor(name);
    let value = crate::locals::slot_cell(slot).borrow().clone();
    let cell = binding_cell(name, slot, Some(&value));
    // A function declaration only creates a new enumerable property when the
    // name is absent.  Eval-time Annex B declarations must preserve the
    // descriptor of an existing configurable property (including a deliberate
    // non-enumerable flag) while replacing its value.
    let descriptor = match current {
        Some(current) if !current.configurable => value_descriptor(cell),
        Some(current) => descriptor_with_flags(cell, &current),
        _ => data_descriptor(cell, true, true, deletable),
    };
    define_global(registers, name, descriptor)
}

fn binding_cell(name: &str, slot: u16, value: Option<&Value>) -> Rc<crate::value::BindingCell> {
    let slot_cell = crate::locals::slot_cell(slot);
    let cell = raw_binding_cell(name).unwrap_or_else(|| Rc::clone(&slot_cell));
    if let Some(value) = value {
        if matches!(*slot_cell.borrow(), Value::Undefined) {
            cell.store(value.clone());
        }
    }
    crate::locals::install_slot_cell(slot, Rc::clone(&cell));
    let realm = crate::vm::current_context_or_default().realm();
    GLOBAL_BINDINGS.with(|bindings| {
        bindings
            .borrow_mut()
            .insert((realm, name.to_string()), Rc::clone(&cell));
    });
    cell
}

fn raw_binding_cell(name: &str) -> Option<Rc<crate::value::BindingCell>> {
    let Value::Object(properties) = crate::vm::current_global_object() else {
        return None;
    };
    let result = properties.iter().rev().find_map(|(key, value)| {
        if key != name {
            return None;
        }
        match value {
            Value::BindingCell(cell) => Some(Rc::clone(&cell)),
            _ => None,
        }
    });
    result
}

fn own_descriptor(name: &str) -> Option<Descriptor> {
    let global = crate::vm::current_global_object();
    let descriptor =
        crate::builtins::object::descriptor(Some(&global), Some(&Value::String(name.to_string())))
            .unwrap_or(Value::Undefined);
    let Value::Object(fields) = descriptor else {
        return immutable_descriptor(name);
    };
    let immutable = crate::globals::immutable_value(name).is_some();
    Some(Descriptor {
        value: field(fields.as_ref(), "value").unwrap_or(Value::Undefined),
        writable: !immutable && flag(fields.as_ref(), "writable"),
        enumerable: !immutable && flag(fields.as_ref(), "enumerable"),
        configurable: !immutable && flag(fields.as_ref(), "configurable"),
        data: has_field(fields.as_ref(), "value") || has_field(fields.as_ref(), "writable"),
    })
}

fn immutable_descriptor(name: &str) -> Option<Descriptor> {
    Some(Descriptor {
        value: crate::globals::immutable_value(name)?,
        writable: false,
        enumerable: false,
        configurable: false,
        data: true,
    })
}

fn value_descriptor(cell: Rc<crate::value::BindingCell>) -> Vec<(String, Value)> {
    vec![("value".to_string(), Value::BindingCell(cell))]
}

fn descriptor_with_flags(
    cell: Rc<crate::value::BindingCell>,
    current: &Descriptor,
) -> Vec<(String, Value)> {
    data_descriptor(
        cell,
        current.writable,
        current.enumerable,
        current.configurable,
    )
}

fn data_descriptor(
    cell: Rc<crate::value::BindingCell>,
    writable: bool,
    enumerable: bool,
    configurable: bool,
) -> Vec<(String, Value)> {
    vec![
        ("value".to_string(), Value::BindingCell(cell)),
        ("writable".to_string(), Value::Boolean(writable)),
        ("enumerable".to_string(), Value::Boolean(enumerable)),
        ("configurable".to_string(), Value::Boolean(configurable)),
    ]
}

fn define_global(
    registers: &mut crate::register_file::RegisterFile,
    name: &str,
    descriptor: Vec<(String, Value)>,
) -> Result<(), VmError> {
    crate::vm::begin_global_declaration_batch();
    if crate::vm::is_global_declaration_batch_active()
        && crate::vm::define_global_declaration_property(name, &descriptor)
    {
        return Ok(());
    }
    let global = crate::vm::current_global_object();
    let updated = crate::builtins::define_own_property(&global, name, &descriptor)?;
    if crate::vm::is_global_declaration_batch_active() {
        crate::vm::update_global_declaration_batch(&updated);
        return Ok(());
    }
    crate::vm::synchronize_global_object(registers, &global, &updated);
    Ok(())
}

fn field<P: crate::value::PropertyEntries + ?Sized>(fields: &P, name: &str) -> Option<Value> {
    fields
        .entries()
        .rev()
        .find_map(|(key, value)| (key == name).then(|| value.clone()))
}

fn has_field<P: crate::value::PropertyEntries + ?Sized>(fields: &P, name: &str) -> bool {
    fields.entries().any(|(key, _)| key == name)
}

fn flag<P: crate::value::PropertyEntries + ?Sized>(fields: &P, name: &str) -> bool {
    matches!(field(fields, name), Some(Value::Boolean(true)))
}
