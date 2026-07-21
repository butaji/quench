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

/// An environment frame that holds variable bindings
pub struct Scope {
    bindings: HashMap<String, Rc<Value>>,
    /// Track variables that are declared but not initialized (var hoisting / TDZ)
    declarations: HashMap<String, VarState>,
    /// Track var kinds for const enforcement
    var_kinds: HashMap<String, VarKind>,
    this_value: Option<Value>,
    /// Whether `this` has been initialized for this scope. Per ES §8.1.1.3.1
    /// BindThisValue and §12.3.5.1 SuperCall: once initialized, a second
    /// super()/this binding should throw ReferenceError. We track it on
    /// the scope that holds the constructor's `this`.
    this_initialized: bool,
    object_binding: Option<Rc<RefCell<crate::value::Object>>>,
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
        }
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
        self.bindings.insert(name.to_string(), Rc::new(value));
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
        match kind {
            VarKind::Var => {
                self.declarations.insert(name, VarState::DeclaredOnly);
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
    pub fn bindings_mut(&mut self) -> &mut HashMap<String, Rc<Value>> {
        &mut self.bindings
    }

    /// Whether this scope has zero bindings and zero declarations.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty() && self.declarations.is_empty()
    }

    pub fn initialize_declared(&mut self, name: &str, value: Value) {
        self.declarations.remove(name);
        self.bindings.insert(name.to_string(), Rc::new(value));
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        if let Some(VarState::DeclaredOnly) = self.declarations.get(name) {
            return Some(Value::Undefined);
        }
        if matches!(self.declarations.get(name), Some(VarState::TDZ)) {
            return None;
        }
        self.bindings.get(name).map(|v| v.as_ref().clone())
    }

    pub fn get_rc(&self, name: &str) -> Option<Rc<Value>> {
        self.bindings.get(name).map(Rc::clone)
    }

    pub fn set(&mut self, name: String, value: Value, strict: bool) -> bool {
        if matches!(self.var_kinds.get(&name), Some(VarKind::Const)) {
            return false;
        }
        match self.bindings.entry(name.clone()) {
            std::collections::hash_map::Entry::Occupied(mut e) => {
                if strict {
                    if let Some(ref obj) = self.object_binding {
                        if let Some(flags) = obj.borrow().get_descriptor(&name) {
                            if !flags.writable {
                                return false;
                            }
                        }
                    }
                }
                e.insert(Rc::new(value));
                true
            }
            std::collections::hash_map::Entry::Vacant(_) => false,
        }
    }

    pub fn define(&mut self, name: String, value: Value) {
        self.declarations.remove(&name);
        self.bindings.insert(name, Rc::new(value));
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

    pub fn bindings(&self) -> impl Iterator<Item = (&String, &Rc<Value>)> {
        self.bindings.iter()
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
