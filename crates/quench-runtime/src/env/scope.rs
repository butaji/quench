//! Scope — an environment frame holding variable bindings.
//! Extracted from env.rs to satisfy the 500-line-per-file linter limit.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;

use crate::ast::VarKind;
use crate::eval::object::{
    call_proxy_set_trap, proxy_get_property, proxy_handler_and_target, proxy_has_property,
};
use crate::value::error::set_thrown_value;
use crate::value::get_thrown_value;
use crate::value::{to_bool, Value};

/// Whether a variable was declared (hoisting support) but not yet initialized
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum VarState {
    /// Variable is declared with a value (may be undefined)
    Initialized(Rc<Value>),
    /// Variable was declared with `var` but initialization hasn't been evaluated yet
    DeclaredOnly,
    /// Variable is in the Temporal Dead Zone (TDZ) - declared but not yet initialized
    TDZ,
}

/// An environment frame that holds variable bindings.
/// `bindings` stores `Rc<RefCell<Value>>` so that multiple scopes (e.g. a for-loop
/// head scope and its per-iteration scopes) can share the same binding storage —
/// updates through one scope's binding are immediately visible through any other
/// scope that holds the same `Rc<RefCell<Value>>`.
pub struct Scope {
    bindings: HashMap<String, Rc<RefCell<Value>>>,
    /// Track variables that are declared but not initialized (var hoisting / TDZ)
    declarations: HashMap<String, VarState>,
    /// Track var kinds for const enforcement
    var_kinds: HashMap<String, VarKind>,
    deletable_bindings: HashSet<String>,
    function_names: HashSet<String>,
    this_value: Option<Value>,
    /// Whether `this` has been initialized for this scope.
    this_initialized: bool,
    object_binding: Option<Rc<RefCell<crate::value::Object>>>,
    with_environment: bool,
    with_unscopables: RefCell<HashSet<String>>,
    with_unscopables_loaded: Cell<bool>,
    /// Marker for static class body scope.
    is_static_class_body: bool,
    /// When set, `set` operations skip this scope. Used for for-loop per-iteration
    /// scopes so that `++i` in the body targets the head binding, not the PI binding.
    per_iteration_scope: bool,
}

impl std::fmt::Debug for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scope")
            .field("bindings", &self.bindings.keys().collect::<Vec<_>>())
            .field(
                "declarations",
                &self.declarations.keys().collect::<Vec<_>>(),
            )
            .field("has_this", &self.this_value.is_some())
            .finish()
    }
}

impl Clone for Scope {
    fn clone(&self) -> Self {
        Scope {
            bindings: self
                .bindings
                .iter()
                .map(|(k, v)| (k.clone(), Rc::clone(v)))
                .collect(),
            declarations: self.declarations.clone(),
            var_kinds: self.var_kinds.clone(),
            deletable_bindings: self.deletable_bindings.clone(),
            function_names: self.function_names.clone(),
            this_value: self.this_value.clone(),
            this_initialized: self.this_initialized,
            object_binding: self.object_binding.as_ref().map(Rc::clone),
            with_environment: self.with_environment,
            with_unscopables: self.with_unscopables.clone(),
            with_unscopables_loaded: Cell::new(self.with_unscopables_loaded.get()),
            is_static_class_body: self.is_static_class_body,
            per_iteration_scope: self.per_iteration_scope,
        }
    }
}

impl Scope {
    pub fn new() -> Self {
        Scope {
            bindings: HashMap::new(),
            declarations: HashMap::new(),
            var_kinds: HashMap::new(),
            deletable_bindings: HashSet::new(),
            function_names: HashSet::new(),
            this_value: None,
            this_initialized: false,
            object_binding: None,
            with_environment: false,
            with_unscopables: RefCell::new(HashSet::new()),
            with_unscopables_loaded: Cell::new(false),
            is_static_class_body: false,
            per_iteration_scope: false,
        }
    }

    fn load_with_unscopables(&self) -> bool {
        if !self.with_environment {
            return true;
        }
        if self.with_unscopables_loaded.get() {
            return true;
        }
        let before = self
            .object_binding
            .as_ref()
            .map(|obj| obj.borrow().own_property_names())
            .unwrap_or_default();
        let blocked = if let Some(ref obj) = self.object_binding {
            let unscopables_val = if let Some(symbol) =
                crate::builtins::symbol::get_well_known_symbol_no_ctx("unscopables")
            {
                match crate::eval::member::eval_object_member_value(obj, &symbol, None) {
                    Ok(v) => Some(v),
                    Err(err) => {
                        if get_thrown_value().is_none() {
                            set_thrown_value(
                                crate::value::create_js_error_with_type(&err.0, "TypeError").0,
                            );
                        }
                        return false;
                    }
                }
            } else {
                None
            };
            match unscopables_val {
                Some(Value::Object(u_obj)) => {
                    let mut blocked = HashSet::new();
                    let names = u_obj.borrow().own_property_names();
                    for name in names {
                        let value = match crate::eval::member::eval_object_member_value(
                            &u_obj,
                            &Value::String(name.clone()),
                            None,
                        ) {
                            Ok(v) => v,
                            Err(err) => {
                                if get_thrown_value().is_none() {
                                    set_thrown_value(
                                        crate::value::create_js_error_with_type(
                                            &err.0,
                                            "TypeError",
                                        )
                                        .0,
                                    );
                                }
                                return false;
                            }
                        };
                        if to_bool(&value) {
                            blocked.insert(name);
                        }
                    }
                    blocked
                }
                _ => HashSet::new(),
            }
        } else {
            HashSet::new()
        };
        let mut blocked = blocked;
        if let Some(obj) = &self.object_binding {
            let object = obj.borrow();
            for name in before {
                if !object.has(&name) {
                    blocked.insert(format!("\0{name}"));
                }
            }
        }
        *self.with_unscopables.borrow_mut() = blocked;
        self.with_unscopables_loaded.set(true);
        true
    }

    /// Mark this scope as a per-iteration scope for for-loop let bindings.
    /// Assignment (`set`) will skip scopes marked this way.
    pub fn mark_per_iteration(&mut self) {
        self.per_iteration_scope = true;
    }

    pub fn clear_per_iteration(&mut self) {
        self.per_iteration_scope = false;
    }

    /// Check if this scope is a per-iteration scope.
    pub fn is_per_iteration(&self) -> bool {
        self.per_iteration_scope
    }

    fn object_has_binding_property(&self, name: &str) -> Option<bool> {
        let obj = self.object_binding.as_ref()?;
        proxy_has_property(obj, name).ok()
    }

    pub fn object_binding_has(&self, name: &str) -> Option<bool> {
        self.object_binding.as_ref()?;
        let has_binding = self.object_has_binding_property(name)?;
        if !has_binding {
            if self.was_deleted_during_unscopables(name) {
                return Some(false);
            }
            return if self.has(name) { Some(false) } else { None };
        }
        if !self.with_environment {
            return Some(true);
        }
        if !self.load_with_unscopables() {
            return None;
        }
        if self.was_deleted_during_unscopables(name) {
            return Some(false);
        }
        Some(!self.is_unscopable(name))
    }

    pub fn is_object_binding(&self) -> bool {
        self.object_binding.is_some()
    }

    pub fn create_object_binding_property(&self, name: &str, value: Value) {
        if let Some(object) = &self.object_binding {
            object.borrow_mut().define(
                name,
                value.clone(),
                crate::value::PropertyFlags {
                    value: Some(value),
                    writable: true,
                    enumerable: true,
                    configurable: true,
                },
            );
        }
    }

    pub fn is_with_environment(&self) -> bool {
        self.with_environment
    }

    pub fn with_base_object(&self) -> Option<Rc<RefCell<crate::value::Object>>> {
        self.object_binding.clone()
    }

    pub fn get_object_binding_value_once(&self, name: &str) -> Option<Value> {
        let object = self.object_binding.as_ref()?;
        if self.object_binding_has(name) != Some(true) || self.is_unscopable(name) {
            return None;
        }
        proxy_get_property(object, name).ok()
    }

    pub fn mark_function_name(&mut self, name: String) {
        self.function_names.insert(name);
    }

    pub fn is_function_name(&self, name: &str) -> bool {
        self.function_names.contains(name)
    }

    pub fn is_global_object_binding(&self) -> bool {
        self.object_binding
            .as_ref()
            .is_some_and(|object| object.borrow().kind == crate::value::ObjectKind::Global)
    }

    pub fn set_object_binding(&mut self, object: Rc<RefCell<crate::value::Object>>) {
        self.object_binding = Some(object);
    }

    pub fn set_with_object_binding(&mut self, object: Rc<RefCell<crate::value::Object>>) {
        self.object_binding = Some(object);
        self.with_environment = true;
        self.clear_with_unscopables();
    }

    pub fn set_with_unscopables(&mut self, blocked: HashSet<String>) {
        *self.with_unscopables.borrow_mut() = blocked;
        self.with_unscopables_loaded.set(true);
    }

    pub fn clear_with_unscopables(&mut self) {
        self.with_unscopables.borrow_mut().clear();
        self.with_unscopables_loaded.set(false);
    }

    pub(crate) fn is_unscopable(&self, name: &str) -> bool {
        let _ = self.load_with_unscopables();
        self.with_unscopables.borrow().contains(name)
    }

    fn was_deleted_during_unscopables(&self, name: &str) -> bool {
        self.with_unscopables
            .borrow()
            .contains(&format!("\0{name}"))
    }

    pub fn is_static_class_body(&self) -> bool {
        self.is_static_class_body
    }

    pub fn set_static_class_body(&mut self) {
        self.is_static_class_body = true;
    }

    pub fn set_object_property(&self, name: &str, value: Value, _strict: bool) -> Option<bool> {
        let object = self.object_binding.as_ref()?.clone();
        if !matches!(self.object_has_binding_property(name), Some(true)) {
            if self.was_deleted_during_unscopables(name) {
                object.borrow_mut().set(name, value);
                return Some(true);
            }
            return None;
        }
        if !self.load_with_unscopables() {
            return None;
        }
        if self.is_unscopable(name) {
            return None;
        }
        if matches!(object.borrow().get_descriptor(name), Some(flags) if !flags.writable) {
            return Some(false);
        }
        let set_ok = if let Some((handler, target)) = proxy_handler_and_target(&object) {
            let success = match call_proxy_set_trap(
                &target,
                &handler,
                &Value::Object(Rc::clone(&object)),
                name,
                value.clone(),
            ) {
                Ok(v) => v,
                Err(_) => return Some(false),
            };
            if !success {
                return Some(false);
            }
            true
        } else {
            object.borrow_mut().set(name, value.clone());
            true
        };
        if !set_ok {
            return Some(false);
        }
        if let Some(binding) = self.bindings.get(name) {
            *binding.borrow_mut() = value;
        }
        Some(true)
    }

    pub fn set_object_property_after_get(
        &self,
        name: &str,
        value: Value,
        _strict: bool,
    ) -> Option<bool> {
        let object = self.object_binding.as_ref()?.clone();
        if !self.load_with_unscopables() || self.is_unscopable(name) {
            return None;
        }
        let is_proxy = proxy_handler_and_target(&object).is_some();
        if !is_proxy && !object.borrow().has(name) {
            return if _strict { Some(false) } else { None };
        }
        if matches!(object.borrow().get_descriptor(name), Some(flags) if !flags.writable) {
            return Some(false);
        }
        if let Some((handler, target)) = proxy_handler_and_target(&object) {
            if proxy_has_property(&object, name).ok() != Some(true) {
                return None;
            }
            return call_proxy_set_trap(
                &target,
                &handler,
                &Value::Object(Rc::clone(&object)),
                name,
                value,
            )
            .ok()
            .map(|success| success);
        }
        object.borrow_mut().set(name, value);
        Some(true)
    }

    pub fn delete_object_property(&mut self, name: &str) -> Option<bool> {
        let object = self.object_binding.as_ref()?;
        if !matches!(self.object_has_binding_property(name), Some(true)) {
            return None;
        }
        if !self.load_with_unscopables() {
            return None;
        }
        if self.is_unscopable(name) {
            return None;
        }
        if let Some(flags) = object.borrow().get_descriptor(name) {
            if !flags.writable {
                return None;
            }
        }
        Some(object.borrow_mut().delete(name))
    }

    pub fn is_tdz(&self, name: &str) -> bool {
        matches!(self.declarations.get(name), Some(VarState::TDZ))
    }

    pub fn mark_tdz(&mut self, name: String) {
        self.var_kinds.insert(name.clone(), VarKind::Let);
        self.declarations.insert(name, VarState::TDZ);
    }

    pub fn declare_var(&mut self, name: String, kind: VarKind) {
        self.var_kinds.insert(name.clone(), kind);
        // If the name is already bound (from `define`) or already declared
        // (from `hoist_classes` or earlier `predeclare_var`), don't shadow it
        // with a new declaration entry.
        // Per ES spec §10.2.11 step 28c: `var arguments` in a function body
        // must not shadow the built-in arguments binding.
        // Also per §15.3: var hoisting must not shadow earlier class/function
        // hoisting.
        if self.bindings.contains_key(&name) || self.declarations.contains_key(&name) {
            return;
        }
        match kind {
            VarKind::Var => {
                self.declarations
                    .insert(name.clone(), VarState::DeclaredOnly);
                // Create property on the global object for var declarations.
                // Per spec §15.1.8: writable=true, enumerable=true, configurable=false.
                // Enumerable ensures for...in can see it. Configurable=false ensures
                // delete returns false for var-declared bindings.
                if let Some(ref obj) = self.object_binding {
                    if !proxy_has_property(obj, &name).unwrap_or(false) {
                        let mut obj_mut = obj.borrow_mut();
                        obj_mut.define(
                            &name,
                            Value::Undefined,
                            crate::value::object::PropertyFlags {
                                value: Some(Value::Undefined),
                                writable: true,
                                enumerable: true,
                                configurable: false,
                            },
                        );
                    }
                }
            }
            VarKind::Let | VarKind::Const => {
                self.declarations.insert(name, VarState::TDZ);
            }
        }
    }

    pub fn declare_eval_var(&mut self, name: String) {
        let new_global_property = self
            .object_binding
            .as_ref()
            .is_some_and(|object| object.borrow().get_own_property(&name).is_none());
        self.deletable_bindings.insert(name.clone());
        self.declare_var(name.clone(), VarKind::Var);
        if new_global_property {
            if let Some(object) = &self.object_binding {
                object.borrow_mut().define(
                    &name,
                    Value::Undefined,
                    crate::value::object::PropertyFlags {
                        value: Some(Value::Undefined),
                        writable: true,
                        enumerable: true,
                        configurable: true,
                    },
                );
            }
        }
    }

    pub fn is_deletable_binding(&self, name: &str) -> bool {
        self.deletable_bindings.contains(name)
    }

    pub fn declare_with_var(&mut self, name: String, kind: VarKind) {
        self.var_kinds.insert(name.clone(), kind);
        if self.bindings.contains_key(&name) || self.declarations.contains_key(&name) {
            return;
        }
        self.declarations.insert(name, VarState::DeclaredOnly);
    }

    pub fn is_declared_only(&self, name: &str) -> bool {
        matches!(
            self.declarations.get(name),
            Some(VarState::DeclaredOnly) | Some(VarState::TDZ)
        )
    }

    pub fn get_kind(&self, name: &str) -> Option<VarKind> {
        self.var_kinds.get(name).copied()
    }

    /// Whether a declaration entry exists (var/let/const declared but not yet initialized).
    pub fn has_declaration(&self, name: &str) -> bool {
        self.declarations.contains_key(name)
    }

    /// Mutable access to bindings (for Environment::set_property).
    pub fn bindings_mut(&mut self) -> &mut HashMap<String, Rc<RefCell<Value>>> {
        &mut self.bindings
    }

    /// Whether this scope has zero bindings and zero declarations.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty() && self.declarations.is_empty()
    }

    pub fn initialize_declared(&mut self, name: &str, value: Value) {
        self.declarations.remove(name);
        self.bindings
            .insert(name.to_string(), Rc::new(RefCell::new(value)));
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        if let Some(VarState::DeclaredOnly) = self.declarations.get(name) {
            if let Some(ref obj) = self.object_binding {
                match self.object_binding_has(name) {
                    Some(false) => {
                        if crate::interpreter::is_strict_mode() {
                            return None;
                        }
                        return Some(Value::Undefined);
                    }
                    Some(true) => {
                        if !matches!(self.object_has_binding_property(name), Some(true)) {
                            if crate::interpreter::is_strict_mode() {
                                return None;
                            }
                            return Some(Value::Undefined);
                        }
                        return match proxy_get_property(obj, name) {
                            Ok(v) => Some(v),
                            Err(_) => Some(Value::Undefined),
                        };
                    }
                    None => return None,
                }
            }
            return Some(Value::Undefined);
        }
        if matches!(self.declarations.get(name), Some(VarState::TDZ)) {
            return None;
        }
        if let Some(value) = self.bindings.get(name) {
            return Some(value.borrow().clone());
        }
        if let Some(object) = self.object_binding.as_ref() {
            let has_binding = self.object_binding_has(name);
            if has_binding == Some(false)
                && self.was_deleted_during_unscopables(name)
                && !crate::interpreter::is_strict_mode()
            {
                return Some(Value::Undefined);
            }
            if has_binding.is_some_and(|has| has) {
                if self.is_unscopable(name) {
                    return None;
                }
                if !matches!(self.object_has_binding_property(name), Some(true)) {
                    return if crate::interpreter::is_strict_mode() {
                        None
                    } else {
                        Some(Value::Undefined)
                    };
                }
                return match proxy_get_property(object, name) {
                    Ok(v) => Some(v),
                    Err(_) => Some(Value::Undefined),
                };
            }
        }
        None
    }

    pub fn get_rc(&self, name: &str) -> Option<Rc<RefCell<Value>>> {
        self.bindings.get(name).map(Rc::clone)
    }

    pub fn set(&mut self, name: String, value: Value, strict: bool) -> bool {
        if matches!(self.var_kinds.get(&name), Some(VarKind::Const)) {
            return false;
        }
        // Handle DeclaredOnly → bindings transition: this happens when a `var`
        // binding without an initializer (e.g. `var obj;`) is first assigned.
        // Also sync to the object_binding (global object) so that subsequent
        // strict-mode assignments don't incorrectly throw ReferenceError
        // (the object_binding_has check would see the binding in bindings but
        // not on the global object, and report Some(false), triggering a throw).
        if self.declarations.contains_key(&name)
            && matches!(self.declarations.get(&name), Some(VarState::DeclaredOnly))
        {
            if self.object_binding.is_some()
                && !matches!(self.object_has_binding_property(&name), Some(true))
            {
                return false;
            }
            self.declarations.remove(&name);
            // Clone the RefCell Rc so all scopes sharing this binding see the update.
            let rc = Rc::new(RefCell::new(value.clone()));
            self.bindings.insert(name.clone(), Rc::clone(&rc));
            // Sync to the global object so the binding is visible there too.
            if let Some(ref obj) = self.object_binding {
                obj.borrow_mut().set(&name, value);
            }
            return true;
        }
        match self.bindings.entry(name.clone()) {
            std::collections::hash_map::Entry::Occupied(e) => {
                if strict {
                    if let Some(ref obj) = self.object_binding {
                        if let Some(flags) = obj.borrow().get_descriptor(&name) {
                            if !flags.writable {
                                return false;
                            }
                        }
                    }
                }
                if matches!(self.var_kinds.get(&name), Some(VarKind::Var)) {
                    if let Some(ref obj) = self.object_binding {
                        obj.borrow_mut().set(&name, value.clone());
                    }
                }
                // Mutate the existing RefCell so all scopes sharing this binding see the update.
                *e.get().borrow_mut() = value;
                true
            }
            std::collections::hash_map::Entry::Vacant(e) => {
                // Check if the global object has this property (e.g. , )
                if let Some(ref obj) = self.object_binding {
                    if proxy_has_property(obj, &name).unwrap_or(false) {
                        // Check writability before setting
                        let writable = obj
                            .borrow()
                            .get_descriptor(&name)
                            .map(|f| f.writable)
                            .unwrap_or(true);
                        if writable || !strict {
                            obj.borrow_mut().set(&name, value.clone());
                            e.insert(Rc::new(RefCell::new(value)));
                            return true;
                        }
                        return false;
                    }
                }
                false
            }
        }
    }

    pub fn define(&mut self, name: String, value: Value) {
        self.declarations.remove(&name);
        self.bindings.insert(name, Rc::new(RefCell::new(value)));
    }

    pub fn define_shared(&mut self, name: String, value: Value) -> Rc<RefCell<Value>> {
        self.declarations.remove(&name);
        let cell = Rc::new(RefCell::new(value));
        self.bindings.insert(name, Rc::clone(&cell));
        cell
    }

    pub fn has(&self, name: &str) -> bool {
        self.bindings.contains_key(name) || self.declarations.contains_key(name)
    }

    /// Remove a binding from this scope. Returns true if the binding existed.
    pub fn delete(&mut self, name: &str) -> bool {
        self.bindings.remove(name).is_some()
    }

    pub fn remove_binding(&mut self, name: &str) {
        self.bindings.remove(name);
        self.declarations.remove(name);
        self.var_kinds.remove(name);
        self.deletable_bindings.remove(name);
        self.function_names.remove(name);
    }

    pub fn get_this(&self) -> Option<Value> {
        self.this_value.clone()
    }

    pub fn set_this(&mut self, value: Value) {
        self.this_value = Some(value);
        self.this_initialized = true;
    }

    pub fn set_this_value(&mut self, value: Value) {
        self.this_value = Some(value);
    }

    pub fn mark_this_initialized(&mut self) {
        self.this_initialized = true;
    }

    pub fn is_this_initialized(&self) -> bool {
        self.this_initialized
    }

    pub fn bindings(&self) -> impl Iterator<Item = (&String, &Rc<RefCell<Value>>)> {
        self.bindings.iter()
    }

    /// Debug dump: print all bindings, declarations, and object properties.
    #[cfg(debug_assertions)]
    pub fn dump(&self, label: &str) {
        eprintln!("[Scope {}] bindings:", label);
        for (k, v) in &self.bindings {
            eprintln!("  {} = {:?}", k, v.borrow());
        }
        eprintln!("[Scope {}] declarations:", label);
        for (k, v) in &self.declarations {
            eprintln!("  {} = {:?}", k, v);
        }
        eprintln!("[Scope {}] var_kinds:", label);
        for (k, v) in &self.var_kinds {
            eprintln!("  {} = {:?}", k, v);
        }
        if let Some(ref obj) = self.object_binding {
            eprintln!(
                "[Scope {}] object_binding keys: {:?}",
                label,
                obj.borrow().properties.keys().collect::<Vec<_>>()
            );
        }
    }
}

impl Default for Scope {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::VarKind;
    use crate::value::Value;
    use crate::Context;

    #[test]
    fn test_scope_new_is_empty() {
        let scope = Scope::new();
        assert!(scope.is_empty());
        assert!(scope.bindings.is_empty());
        assert!(scope.declarations.is_empty());
    }

    #[test]
    fn test_scope_define_and_get() {
        let mut scope = Scope::new();
        scope.define("x".to_string(), Value::Number(42.0));
        assert!(!scope.is_empty());
        assert_eq!(scope.get("x"), Some(Value::Number(42.0)));
    }

    #[test]
    fn test_scope_get_missing() {
        let scope = Scope::new();
        assert_eq!(scope.get("missing"), None);
    }

    #[test]
    fn test_scope_set_existing() {
        let mut scope = Scope::new();
        scope.define("x".to_string(), Value::Number(1.0));
        assert!(scope.set("x".to_string(), Value::Number(2.0), false));
        assert_eq!(scope.get("x"), Some(Value::Number(2.0)));
    }

    #[test]
    fn test_scope_set_missing_returns_false() {
        let mut scope = Scope::new();
        assert!(!scope.set("x".to_string(), Value::Number(1.0), false));
    }

    #[test]
    fn with_scope_proxy_has_throw_propagates_from_lookup() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval(
            r#"var log = [];
             var proxy = new Proxy({}, {
               has(t, pk) {
                 if (pk === 'Object') {
                   throw new Error("has-throw");
                 }
                 return true;
               },
               get(t, pk) {
                 return Reflect.get(t, pk);
               },
             });
             with (proxy) { Object; }
             log.join(',');"#,
        );
        let err = result.expect_err("expected proxy has trap throw");
        assert!(err.to_string().contains("has-throw"));
    }

    #[test]
    fn with_scope_proxy_missing_binding_does_not_load_unscopables() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval(
            "var log = []; \
             var proxy = new Proxy({}, { \
               has(t, p) { log.push('has:' + String(p)); return p in t; }, \
               get() { throw new Error('unexpected get'); }, \
             }); \
             with (proxy) { Object; } \
             log.join(',');",
        );
        assert_eq!(result.unwrap(), Value::String("has:Object".to_string()));
    }

    #[test]
    fn with_scope_object_binding_has_uses_proxy_has_trap() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval(
                "var log = []; \
                 var proxy = new Proxy({Object}, { \
                   has(t, p) { log.push('has:' + String(p)); return p in t; }, \
                   get(t, p, r) { log.push('get:' + String(p)); return t[p]; }, \
                 }); \
                 with (proxy) { log.push('with-id:'+typeof Object); Object(); } \
                 log.join(',');",
            )
            .unwrap();
        let log = result.to_string();
        assert!(log.contains("get:Symbol(Symbol.unscopables)"));
        assert!(log.contains("has:log"));
        assert!(log.contains("get:Object"));
        assert!(log.contains("with-id:function"));
    }

    #[test]
    fn with_scope_unscopables_getter_throws() {
        let mut ctx = Context::new().unwrap();
        let err = ctx
            .eval(
                "var log = []; \
                 var base = { Object: 1 }; \
                 Object.defineProperty(base, Symbol.unscopables, { \
                   get() { log.push('get-unscopables'); throw new Error('unscopables-threw'); } \
                 }); \
                 with (new Proxy(base, { \
                   has(t, p) { log.push('has:' + String(p)); return p in t; }, \
                   get(t, p) { log.push('get:' + String(p)); return t[p]; }, \
                 })) { Object; }",
            )
            .unwrap_err();
        assert!(err.to_string().contains("unscopables-threw"));
    }

    #[test]
    fn global_object_with_scope_blocks_unscopable_binding() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "globalThis.v = 1; \
             globalThis[Symbol.unscopables] = { v: true };",
        )
        .unwrap();
        let Value::Object(global) = ctx.get_global("globalThis").unwrap() else {
            panic!();
        };
        let mut scope = Scope::new();
        scope.set_with_object_binding(global);
        assert_eq!(scope.object_binding_has("v"), Some(false));
    }

    #[test]
    fn test_scope_const_immutable() {
        let mut scope = Scope::new();
        scope.declare_var("const_val".to_string(), VarKind::Const);
        scope.initialize_declared("const_val", Value::Number(1.0));
        assert!(!scope.set("const_val".to_string(), Value::Number(2.0), false));
        assert_eq!(scope.get("const_val"), Some(Value::Number(1.0)));
    }

    #[test]
    fn test_scope_declare_var_kind() {
        let mut scope = Scope::new();
        scope.declare_var("myvar".to_string(), VarKind::Var);
        scope.declare_var("mylet".to_string(), VarKind::Let);
        scope.declare_var("myconst".to_string(), VarKind::Const);

        assert_eq!(scope.get_kind("myvar"), Some(VarKind::Var));
        assert_eq!(scope.get_kind("mylet"), Some(VarKind::Let));
        assert_eq!(scope.get_kind("myconst"), Some(VarKind::Const));
        assert_eq!(scope.get_kind("missing"), None);
    }

    #[test]
    fn test_scope_tdz() {
        let mut scope = Scope::new();
        scope.mark_tdz("x".to_string());
        assert!(scope.is_tdz("x"));
        assert_eq!(scope.get("x"), None);
        assert!(scope.has_declaration("x"));
    }

    #[test]
    fn test_scope_declared_only() {
        let mut scope = Scope::new();
        scope.declare_var("y".to_string(), VarKind::Var);
        assert!(scope.is_declared_only("y"));
        assert_eq!(scope.get("y"), Some(Value::Undefined));
    }

    #[test]
    fn test_scope_initialize_declared() {
        let mut scope = Scope::new();
        scope.declare_var("z".to_string(), VarKind::Var);
        assert_eq!(scope.get("z"), Some(Value::Undefined));
        scope.initialize_declared("z", Value::Number(99.0));
        assert_eq!(scope.get("z"), Some(Value::Number(99.0)));
    }

    #[test]
    fn test_scope_delete() {
        let mut scope = Scope::new();
        scope.define("x".to_string(), Value::Number(1.0));
        assert!(scope.delete("x"));
        assert_eq!(scope.get("x"), None);
        assert!(!scope.delete("x"));
    }

    #[test]
    fn test_scope_has() {
        let mut scope = Scope::new();
        scope.define("x".to_string(), Value::Number(1.0));
        scope.declare_var("y".to_string(), VarKind::Var);
        assert!(scope.has("x"));
        assert!(scope.has("y"));
        assert!(!scope.has("z"));
    }

    #[test]
    fn test_scope_this_binding() {
        let mut scope = Scope::new();
        assert_eq!(scope.get_this(), None);
        scope.set_this(Value::Number(42.0));
        assert_eq!(scope.get_this(), Some(Value::Number(42.0)));
        assert!(scope.is_this_initialized());
    }

    #[test]
    fn test_scope_set_this_value() {
        let mut scope = Scope::new();
        scope.set_this_value(Value::String("hello".to_string()));
        assert_eq!(scope.get_this(), Some(Value::String("hello".to_string())));
        assert!(!scope.is_this_initialized());
    }

    #[test]
    fn test_scope_clone() {
        let mut scope = Scope::new();
        scope.define("x".to_string(), Value::Number(1.0));
        scope.set_this(Value::Number(42.0));

        let cloned = scope.clone();
        assert_eq!(cloned.get("x"), Some(Value::Number(1.0)));
        assert_eq!(cloned.get_this(), Some(Value::Number(42.0)));
    }

    #[test]
    fn test_scope_debug() {
        let mut scope = Scope::new();
        scope.define("a".to_string(), Value::Number(1.0));
        scope.declare_var("b".to_string(), VarKind::Var);
        let debug = format!("{:?}", scope);
        assert!(debug.contains("a"));
        assert!(debug.contains("b"));
    }

    #[test]
    fn test_scope_bindings_iter() {
        let mut scope = Scope::new();
        scope.define("x".to_string(), Value::Number(1.0));
        scope.define("y".to_string(), Value::Number(2.0));

        let names: Vec<_> = scope.bindings().map(|(k, _)| k.clone()).collect();
        assert!(names.contains(&"x".to_string()));
        assert!(names.contains(&"y".to_string()));
    }

    #[test]
    fn test_scope_object_binding() {
        let mut scope = Scope::new();
        let obj = std::rc::Rc::new(std::cell::RefCell::new(crate::value::Object::new(
            crate::value::ObjectKind::Ordinary,
        )));
        scope.set_object_binding(obj.clone());
        assert!(scope.is_object_binding());
        assert!(scope.object_binding_has("missing").is_none());
    }

    #[test]
    fn define_shared_returns_the_binding_cell() {
        let mut scope = Scope::new();
        let cell = scope.define_shared("shared_cell".to_string(), Value::Number(1.0));
        *cell.borrow_mut() = Value::Number(2.0);
        assert_eq!(scope.get("shared_cell"), Some(Value::Number(2.0)));
    }
}
