//! Scope — an environment frame holding variable bindings.
//! Extracted from env.rs to satisfy the 500-line-per-file linter limit.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::VarKind;
use crate::value::Value;

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
    this_value: Option<Value>,
    /// Whether `this` has been initialized for this scope.
    this_initialized: bool,
    object_binding: Option<Rc<RefCell<crate::value::Object>>>,
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
            this_value: self.this_value.clone(),
            this_initialized: self.this_initialized,
            object_binding: self.object_binding.as_ref().map(Rc::clone),
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
            this_value: None,
            this_initialized: false,
            object_binding: None,
            is_static_class_body: false,
            per_iteration_scope: false,
        }
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

    pub fn object_binding_has(&self, name: &str) -> Option<bool> {
        if !self.bindings.contains_key(name) {
            return None;
        }
        let result = self.object_binding.as_ref()?.borrow().has(name);
        Some(result)
    }

    pub fn is_object_binding(&self) -> bool {
        self.object_binding.is_some()
    }

    pub fn set_object_binding(&mut self, object: Rc<RefCell<crate::value::Object>>) {
        self.object_binding = Some(object);
    }

    pub fn is_static_class_body(&self) -> bool {
        self.is_static_class_body
    }

    pub fn set_static_class_body(&mut self) {
        self.is_static_class_body = true;
    }

    pub fn set_object_property(&mut self, name: &str, value: Value, _strict: bool) -> Option<bool> {
        let object = self.object_binding.as_ref()?;
        if !self.bindings.contains_key(name) {
            return None;
        }
        if !object.borrow().has(name) {
            return None;
        }
        if let Some(flags) = object.borrow().get_descriptor(name) {
            if !flags.writable {
                return None;
            }
        }
        object.borrow_mut().set(name, value.clone());
        *self.bindings.get(name).unwrap().borrow_mut() = value;
        Some(true)
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
                    if !obj.borrow().has(&name) {
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
                if obj.borrow().has(name) {
                    return obj.borrow().get(name);
                }
            }
            return Some(Value::Undefined);
        }
        if matches!(self.declarations.get(name), Some(VarState::TDZ)) {
            return None;
        }
        self.bindings.get(name).map(|v| v.borrow().clone())
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
                // Mutate the existing RefCell so all scopes sharing this binding see the update.
                *e.get().borrow_mut() = value;
                true
            }
            std::collections::hash_map::Entry::Vacant(e) => {
                // Check if the global object has this property (e.g. , )
                if let Some(ref obj) = self.object_binding {
                    if obj.borrow().has(&name) {
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

    pub fn has(&self, name: &str) -> bool {
        self.bindings.contains_key(name) || self.declarations.contains_key(name)
    }

    /// Remove a binding from this scope. Returns true if the binding existed.
    pub fn delete(&mut self, name: &str) -> bool {
        self.bindings.remove(name).is_some()
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
}
