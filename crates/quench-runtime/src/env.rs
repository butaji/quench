//! Environment (scope chain) for the JavaScript interpreter

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
}

impl std::fmt::Debug for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Print bindings keys only to avoid potential recursion with Values
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
        }
    }

    /// Check if a variable is in TDZ state
    pub fn is_tdz(&self, name: &str) -> bool {
        matches!(self.declarations.get(name), Some(VarState::TDZ))
    }

    /// Mark a variable as in TDZ (for let/const declarations)
    pub fn mark_tdz(&mut self, name: String) {
        self.var_kinds.insert(name.clone(), VarKind::Let);
        self.declarations.insert(name, VarState::TDZ);
    }

    /// Mark a variable as declared-only (for var hoisting)
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

    /// Check if a variable is declared but not yet initialized
    pub fn is_declared_only(&self, name: &str) -> bool {
        matches!(
            self.declarations.get(name),
            Some(VarState::DeclaredOnly) | Some(VarState::TDZ)
        )
    }

    /// Get the kind of a variable
    pub fn get_kind(&self, name: &str) -> Option<VarKind> {
        self.var_kinds.get(name).copied()
    }

    /// Initialize a declared variable
    pub fn initialize_declared(&mut self, name: &str, value: Value) {
        self.declarations.remove(name);
        self.bindings.insert(name.to_string(), Rc::new(value));
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        // For declared-only (hoisted var), return undefined
        if let Some(VarState::DeclaredOnly) = self.declarations.get(name) {
            return Some(Value::Undefined);
        }
        // For TDZ, return None (will be caught as TDZ error)
        if matches!(self.declarations.get(name), Some(VarState::TDZ)) {
            return None;
        }
        self.bindings.get(name).map(|v| v.as_ref().clone())
    }

    /// Get a reference-counted value (for interpreter use)
    pub fn get_rc(&self, name: &str) -> Option<Rc<Value>> {
        self.bindings.get(name).map(Rc::clone)
    }

    pub fn set(&mut self, name: String, value: Value) -> bool {
        match self.bindings.entry(name) {
            std::collections::hash_map::Entry::Occupied(mut e) => {
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

    /// Get the "this" binding for this scope
    pub fn get_this(&self) -> Option<Value> {
        self.this_value.clone()
    }

    /// Set the "this" binding for this scope
    pub fn set_this(&mut self, value: Value) {
        self.this_value = Some(value);
    }
}

impl Default for Scope {
    fn default() -> Self {
        Self::new()
    }
}

/// An environment holds a scope chain for variable resolution
pub struct Environment {
    pub scopes: Vec<Scope>,
    /// Parent environment (for closures)
    parent: Option<Rc<RefCell<Environment>>>,
    /// Super class reference for class methods/constructors
    super_class: Option<Value>,
}

impl std::fmt::Debug for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Avoid printing parent to prevent infinite recursion
        f.debug_struct("Environment")
            .field("scopes", &self.scopes)
            .field(
                "parent",
                &if self.parent.is_some() { "..." } else { "None" },
            )
            .finish()
    }
}

impl Environment {
    /// Create a new top-level environment
    pub fn new() -> Self {
        Environment {
            scopes: vec![Scope::new()],
            parent: None,
            super_class: None,
        }
    }

    /// Create a new environment with a parent
    pub fn with_parent(parent: Rc<RefCell<Environment>>) -> Self {
        Environment {
            scopes: vec![Scope::new()],
            parent: Some(parent),
            super_class: None,
        }
    }

    /// Set the super class reference for class methods/constructors
    pub fn set_super_class(&mut self, super_class: Value) {
        self.super_class = Some(super_class);
    }

    /// Get the super class reference
    pub fn get_super_class(&self) -> Option<Value> {
        self.super_class.clone()
    }

    /// Get the parent environment
    pub fn get_parent(&self) -> Option<Rc<RefCell<Environment>>> {
        self.parent.clone()
    }

    /// Get a variable by name (lexical lookup)
    /// Returns a cloned Value for simplicity.
    /// Falls back to globalThis if not found in the scope chain.
    pub fn get(&self, name: &str) -> Option<Value> {
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return Some(value);
            }
        }
        // Look up in parent if not found
        if let Some(ref parent) = self.parent {
            return parent.borrow().get(name);
        }
        // At top level, fall back to globalThis properties
        self.get_global_this_property(name)
    }

    /// Get a variable as Rc<Value> for identity preservation.
    /// This ensures function properties persist across multiple accesses.
    pub fn get_shared(&self, name: &str) -> Option<Rc<Value>> {
        for scope in self.scopes.iter().rev() {
            if let Some(rc) = scope.get_rc(name) {
                return Some(rc);
            }
        }
        // Look up in parent if not found
        if let Some(ref parent) = self.parent {
            return parent.borrow().get_shared(name);
        }
        None
    }

    /// Get a property from globalThis if it exists.
    /// This is called as a fallback when the variable is not found in the scope chain.
    fn get_global_this_property(&self, name: &str) -> Option<Value> {
        // Look for globalThis in all environments in the chain (including parents)
        for scope in &self.scopes {
            if let Some(Value::Object(global_obj)) = scope.get("globalThis") {
                if let Some(val) = global_obj.borrow().get(name) {
                    return Some(val);
                }
            }
        }
        // Also check parent environments
        if let Some(ref parent) = self.parent {
            return parent.borrow().get_global_this_property(name);
        }
        None
    }

    /// Get a variable by name, returning an Rc for identity preservation.
    /// For function values, this ensures the same closure is used.
    pub fn get_rc(&self, name: &str) -> Option<Rc<Value>> {
        self.get_shared(name)
    }

    /// Set a property on a variable stored by name (for function properties).
    /// Modifies the Value in place via RefCell to preserve Rc identity.
    /// Returns true if the property was set successfully.
    pub fn set_property(&mut self, name: &str, prop: &str, value: Value) -> bool {
        // Try current scopes first - use get_mut for in-place modification
        for scope in self.scopes.iter_mut().rev() {
            if let std::collections::hash_map::Entry::Occupied(entry) =
                scope.bindings.entry(name.to_string())
            {
                let rc = entry.get();
                match rc.as_ref() {
                    Value::Function(ref f) => {
                        f.set_property(prop, value.clone());
                        return true;
                    }
                    Value::NativeFunction(ref nf) => {
                        nf.set_property(prop, value.clone());
                        return true;
                    }
                    _ => return false,
                }
            }
        }
        // Try parent
        if let Some(ref parent) = self.parent {
            return parent.borrow_mut().set_property(name, prop, value);
        }
        false
    }

    /// Set a variable by name (assigns to existing binding)
    /// If not found in current environment, tries to set in parent.
    pub fn set(&mut self, name: &str, value: Value) -> bool {
        for scope in self.scopes.iter_mut().rev() {
            if scope.set(name.to_string(), value.clone()) {
                return true;
            }
        }
        // Try to set in parent
        if let Some(ref parent) = self.parent {
            return parent.borrow_mut().set(name, value);
        }
        false
    }

    /// Define a new variable in the current (innermost) scope
    pub fn define(&mut self, name: String, value: Value) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.define(name, value);
        }
    }

    /// Declare a variable (same as define, for compatibility)
    pub fn declare(&mut self, name: String, value: Value) {
        self.define(name, value);
    }

    /// Declare a variable with its kind (for var/let/const handling)
    pub fn declare_var(&mut self, name: String, kind: VarKind) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.declare_var(name, kind);
        }
    }

    /// Initialize a declared variable (removes from declarations, adds to bindings)
    /// Finds the innermost scope where the variable was declared.
    pub fn initialize_declared(&mut self, name: &str, value: Value) {
        // Search from innermost scope outward so block-scoped declarations
        // shadow outer declarations, matching JavaScript lexical scoping.
        for (i, scope) in self.scopes.iter().enumerate().rev() {
            if scope.declarations.contains_key(name) {
                self.scopes[i].initialize_declared(name, value);
                return;
            }
        }

        // No pending declaration: the variable was already initialized earlier
        // (e.g. a function-scoped `var` re-initialized on a later loop
        // iteration). Update the existing local binding instead of letting
        // the write escape to the parent environment.
        for scope in self.scopes.iter_mut().rev() {
            if scope.bindings.contains_key(name) {
                scope.set(name.to_string(), value);
                return;
            }
        }

        // Otherwise delegate to the parent environment (declaration or
        // binding may live in a closure scope).
        if let Some(ref parent) = self.parent {
            parent.borrow_mut().initialize_declared(name, value);
        }
    }

    /// Check if a variable is in TDZ in the current scope.
    /// If the innermost scope has any record of the name (binding or
    /// declaration), only that scope's TDZ state matters; an inner binding
    /// shadows any outer TDZ.
    /// Check if a variable is in TDZ in the current scope.
    /// If the innermost scope has any record of the name (binding or
    /// declaration), only that scope's TDZ state matters; an inner binding
    /// shadows any outer TDZ.
    pub fn is_tdz(&self, name: &str) -> bool {
        if let Some(scope) = self.scopes.last() {
            if scope.has(name) {
                return scope.is_tdz(name);
            }
        }
        // Check parent
        if let Some(ref parent) = self.parent {
            return parent.borrow().is_tdz(name);
        }
        false
    }

    /// Get the kind of a variable (Var, Let, Const) by looking up the scope chain
    pub fn get_kind(&self, name: &str) -> Option<VarKind> {
        for scope in self.scopes.iter().rev() {
            if let Some(kind) = scope.get_kind(name) {
                return Some(kind);
            }
        }
        // Check parent
        if let Some(ref parent) = self.parent {
            return parent.borrow().get_kind(name);
        }
        None
    }

    /// Check if a variable exists in any scope or on globalThis
    pub fn has(&self, name: &str) -> bool {
        for scope in &self.scopes {
            if scope.has(name) {
                return true;
            }
        }
        // Look up in parent if not found
        if let Some(ref parent) = self.parent {
            return parent.borrow().has(name);
        }
        // Fall back to globalThis so typeof on a global property works
        self.get_global_this_property(name).is_some()
    }

    /// Push a new scope onto the stack
    pub fn push_scope(&mut self) {
        self.scopes.push(Scope::new());
    }

    /// Pop the current scope from the stack
    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    /// Get the current scope
    pub fn current_scope(&self) -> &Scope {
        self.scopes.last().unwrap()
    }

    /// Get a mutable reference to the current scope
    pub fn current_scope_mut(&mut self) -> &mut Scope {
        self.scopes.last_mut().unwrap()
    }

    /// Get all variable names in the current scope
    pub fn keys(&self) -> Vec<String> {
        self.scopes
            .last()
            .map(|s| s.bindings.keys().cloned().collect())
            .unwrap_or_default()
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for Environment {
    fn clone(&self) -> Self {
        // Preserve the parent chain (shared via Rc) and super_class; dropping
        // them would silently break variable and `super` resolution for any
        // code that clones an environment.
        Environment {
            scopes: self.scopes.clone(),
            parent: self.parent.clone(),
            super_class: self.super_class.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_define_and_get() {
        let mut env = Environment::new();
        env.define("x".to_string(), Value::Number(42.0));

        assert_eq!(env.get("x"), Some(Value::Number(42.0)));
    }

    #[test]
    fn test_scope_chain() {
        let mut env = Environment::new();
        env.define("outer".to_string(), Value::Number(1.0));

        env.push_scope();
        env.define("inner".to_string(), Value::Number(2.0));

        assert_eq!(env.get("inner"), Some(Value::Number(2.0)));
        assert_eq!(env.get("outer"), Some(Value::Number(1.0)));

        env.pop_scope();

        assert_eq!(env.get("inner"), None);
        assert_eq!(env.get("outer"), Some(Value::Number(1.0)));
    }

    #[test]
    fn test_set_existing() {
        let mut env = Environment::new();
        env.define("x".to_string(), Value::Number(1.0));

        assert!(env.set("x", Value::Number(2.0)));
        assert_eq!(env.get("x"), Some(Value::Number(2.0)));
    }

    #[test]
    fn test_has() {
        let mut env = Environment::new();
        env.define("x".to_string(), Value::Number(1.0));

        assert!(env.has("x"));
        assert!(!env.has("y"));
    }
}
