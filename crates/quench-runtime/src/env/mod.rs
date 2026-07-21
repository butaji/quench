//! Environment (scope chain) for the JavaScript interpreter.
//!
//! Canonical types:
//!   - `scope.rs` — `VarState`, `Scope` (an environment frame)
//!   - `mod.rs`   — `Environment` (the scope chain manager)
//!   - `tests.rs` — unit tests

use std::cell::RefCell;
use std::rc::Rc;

mod scope;

use crate::ast::{Expression, PropertyKey, VarKind};
use crate::value::Value;
pub use scope::{Scope, VarState};

// ─── Environment ─────────────────────────────────────────────────────────────

/// An environment holds a scope chain for variable resolution. Each
/// scope lives behind an `Rc<RefCell<Scope>>` so that closures created
/// in the same block (or in nested blocks) can share the SAME scope
/// records as the active environment — writes through one closure are
/// visible to every other closure that captured the same scope. When a
/// scope is logically popped (`pop_scope`), it stays in `scopes` so that
/// closures which captured it via `Rc::clone` can still resolve their
/// bindings, but the live-environment lookup methods skip scopes whose
/// `popped` flag is set.
pub struct Environment {
    pub scopes: Vec<Rc<RefCell<Scope>>>,
    /// Parent environment (for closures)
    parent: Option<Rc<RefCell<Environment>>>,
    /// Super class reference for class methods/constructors
    super_class: Option<Value>,
    /// Pending field initializers for derived class constructors.
    /// Evaluated after super() returns, per ES spec.
    pending_fields: Option<Vec<(PropertyKey, Expression)>>,
}

impl std::fmt::Debug for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Environment")
            .field("scopes", &self.scopes)
            .field(
                "parent",
                &if self.parent.is_some() { "..." } else { "None" },
            )
            .finish()
    }
}

/// Helper: is the named own property writable on a ValueFunction?
/// Per ES §9.2.4 FunctionInitialize, `length` and `name` are non-writable.
fn is_writable_function_prop(key: &str) -> bool {
    !matches!(key, "length" | "name")
}

impl Environment {
    pub fn new() -> Self {
        Environment {
            scopes: vec![Rc::new(RefCell::new(Scope::new()))],
            parent: None,
            super_class: None,
            pending_fields: None,
        }
    }

    pub fn with_parent(parent: Rc<RefCell<Environment>>) -> Self {
        Environment {
            scopes: vec![Rc::new(RefCell::new(Scope::new()))],
            parent: Some(parent),
            super_class: None,
            pending_fields: None,
        }
    }

    pub fn set_parent(&mut self, parent: Rc<RefCell<Environment>>) {
        self.parent = Some(parent);
    }

    pub fn set_super_class(&mut self, super_class: Value) {
        self.super_class = Some(super_class);
    }

    pub fn get_super_class(&self) -> Option<Value> {
        self.super_class.clone()
    }

    pub fn set_pending_fields(&mut self, fields: Vec<(PropertyKey, Expression)>) {
        self.pending_fields = Some(fields);
    }

    pub fn take_pending_fields(&mut self) -> Option<Vec<(PropertyKey, Expression)>> {
        self.pending_fields.take()
    }

    pub fn get_parent(&self) -> Option<Rc<RefCell<Environment>>> {
        self.parent.clone()
    }

    pub fn live_scopes_snapshot(&self) -> Vec<Rc<RefCell<Scope>>> {
        self.scopes.to_vec()
    }

    pub fn capture_env(&self) -> Environment {
        let mut captured = Environment::new();
        captured.scopes = self.live_scopes_snapshot();
        captured.parent = self.parent.clone();
        captured.super_class = self.super_class.clone();
        captured
    }

    pub fn binding_scope(&self, name: &str) -> Option<Rc<RefCell<Scope>>> {
        for scope in self.scopes.iter().rev() {
            if scope.borrow().has(name) {
                return Some(Rc::clone(scope));
            }
        }
        self.parent.as_ref()?.borrow().binding_scope(name)
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        for scope_rc in self.scopes.iter().rev() {
            if let Some(value) = scope_rc.borrow().get(name) {
                return Some(value);
            }
        }
        if let Some(ref parent) = self.parent {
            return parent.borrow().get(name);
        }
        self.get_global_this_property(name)
    }

    pub fn get_shared(&self, name: &str) -> Option<Rc<Value>> {
        for scope_rc in self.scopes.iter().rev() {
            if let Some(rc) = scope_rc.borrow().get_rc(name) {
                return Some(rc);
            }
        }
        if let Some(ref parent) = self.parent {
            return parent.borrow().get_shared(name);
        }
        None
    }

    fn get_global_this_property(&self, name: &str) -> Option<Value> {
        for scope_rc in self.scopes.iter() {
            let scope = scope_rc.borrow();
            if let Some(Value::Object(global_obj)) = scope.get("globalThis") {
                if let Some(val) = global_obj.borrow().get(name) {
                    return Some(val);
                }
            }
        }
        if let Some(ref parent) = self.parent {
            return parent.borrow().get_global_this_property(name);
        }
        None
    }

    pub fn get_rc(&self, name: &str) -> Option<Rc<Value>> {
        self.get_shared(name)
    }

    pub fn set_property(&mut self, name: &str, prop: &str, value: Value) -> bool {
        for scope_rc in self.scopes.iter().rev() {
            let mut scope = scope_rc.borrow_mut();
            if let std::collections::hash_map::Entry::Occupied(entry) =
                scope.bindings_mut().entry(name.to_string())
            {
                let rc = entry.get();
                match rc.as_ref() {
                    Value::Function(ref f) => {
                        if f.is_arrow && (prop == "caller" || prop == "arguments") {
                            return false;
                        }
                        if !crate::interpreter::is_strict_mode()
                            && f.get_property(prop).is_some()
                            && !is_writable_function_prop(prop)
                        {
                            return true;
                        }
                        if crate::interpreter::is_strict_mode()
                            && f.get_property(prop).is_some()
                            && !is_writable_function_prop(prop)
                        {
                            return false;
                        }
                        f.set_property(prop, value.clone());
                        return true;
                    }
                    Value::NativeFunction(ref nf) => {
                        let _ = nf.set_property(prop, value.clone());
                        return true;
                    }
                    _ => return false,
                }
            }
        }
        if let Some(ref parent) = self.parent {
            return parent.borrow_mut().set_property(name, prop, value);
        }
        false
    }

    pub fn set(&mut self, name: &str, value: Value) -> bool {
        for scope_rc in self.scopes.iter().rev() {
            let mut scope = scope_rc.borrow_mut();
            if let Some(success) =
                scope.set_object_property(name, value.clone(), crate::interpreter::is_strict_mode())
            {
                return success;
            }
            if scope.get_kind(name) == Some(VarKind::Var) && scope.is_declared_only(name) {
                scope.initialize_declared(name, value.clone());
                return true;
            }
            if scope.set(
                name.to_string(),
                value.clone(),
                crate::interpreter::is_strict_mode(),
            ) {
                return true;
            }
        }
        if let Some(ref parent) = self.parent {
            return parent.borrow_mut().set(name, value);
        }
        // No binding found in scope chain. In sloppy mode, create implicit global
        // in the global scope (first scope). In strict mode, return false so
        // the caller throws ReferenceError.
        if !crate::interpreter::is_strict_mode() {
            if let Some(global_scope) = self.scopes.first() {
                global_scope
                    .borrow_mut()
                    .bindings_mut()
                    .insert(name.to_string(), Rc::new(value));
                return true;
            }
        }
        false
    }

    pub fn define(&mut self, name: String, value: Value) {
        if let Some(mut scope) = self.current_scope_ref_mut() {
            scope.define(name, value);
        }
    }

    pub fn declare(&mut self, name: String, value: Value) {
        self.define(name, value);
    }

    pub fn declare_var(&mut self, name: String, kind: VarKind) {
        if kind == VarKind::Var {
            if let Some(scope) = self.scopes.first() {
                scope.borrow_mut().declare_var(name, kind);
            }
        } else if let Some(mut scope) = self.current_scope_ref_mut() {
            scope.declare_var(name, kind);
        }
    }

    pub fn initialize_declared(&mut self, name: &str, value: Value) {
        for scope_rc in self.scopes.iter().rev() {
            let mut scope = scope_rc.borrow_mut();
            if scope.has_declaration(name) {
                scope.initialize_declared(name, value);
                return;
            }
        }
        for scope_rc in self.scopes.iter().rev() {
            let mut scope = scope_rc.borrow_mut();
            if scope.has(name) {
                scope.set(
                    name.to_string(),
                    value,
                    crate::interpreter::is_strict_mode(),
                );
                return;
            }
        }
        if let Some(ref parent) = self.parent {
            parent.borrow_mut().initialize_declared(name, value);
        }
    }

    pub fn is_tdz(&self, name: &str) -> bool {
        if let Some(scope) = self.current_scope_ref() {
            if scope.has(name) {
                return scope.is_tdz(name);
            }
        }
        if let Some(ref parent) = self.parent {
            return parent.borrow().is_tdz(name);
        }
        false
    }

    pub fn get_kind(&self, name: &str) -> Option<VarKind> {
        for scope_rc in self.scopes.iter().rev() {
            if let Some(kind) = scope_rc.borrow().get_kind(name) {
                return Some(kind);
            }
        }
        if let Some(ref parent) = self.parent {
            return parent.borrow().get_kind(name);
        }
        None
    }

    pub fn has(&self, name: &str) -> bool {
        for scope_rc in self.scopes.iter() {
            if scope_rc.borrow().has(name) {
                return true;
            }
        }
        if let Some(ref parent) = self.parent {
            return parent.borrow().has(name);
        }
        self.get_global_this_property(name).is_some()
    }

    /// Delete a binding from the innermost scope that has it.
    /// Returns true if an implicit-global binding was found and deleted.
    /// Returns false for declared bindings (var/let/const) or if not found.
    pub fn delete_binding(&mut self, name: &str) -> bool {
        for scope_rc in self.scopes.iter_mut().rev() {
            let mut scope = scope_rc.borrow_mut();
            if scope.has(name) {
                // Don't delete declared bindings (var/let/const) — only implicit globals
                if scope.get_kind(name).is_some() {
                    return false;
                }
                let deleted = scope.delete(name);
                return deleted;
            }
        }
        if let Some(ref parent) = self.parent {
            return parent.borrow_mut().delete_binding(name);
        }
        false
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(Rc::new(RefCell::new(Scope::new())));
    }

    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    pub fn current_scope(&self) -> Rc<RefCell<Scope>> {
        Rc::clone(
            self.scopes
                .last()
                .expect("environment always has at least one scope"),
        )
    }

    fn current_scope_ref(&self) -> Option<std::cell::Ref<'_, Scope>> {
        self.scopes.last().map(|s| s.borrow())
    }

    fn current_scope_ref_mut(&self) -> Option<std::cell::RefMut<'_, Scope>> {
        self.scopes.last().map(|s| s.borrow_mut())
    }

    pub fn keys(&self) -> Vec<String> {
        self.current_scope_ref()
            .map(|s| s.bindings().map(|(k, _)| k.clone()).collect())
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
        Environment {
            scopes: self.scopes.clone(),
            parent: self.parent.clone(),
            super_class: self.super_class.clone(),
            pending_fields: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_live_scopes_snapshot() {
        let mut env = Environment::new();
        env.define("x".to_string(), Value::Number(1.0));
        env.push_scope();
        env.define("y".to_string(), Value::Number(2.0));
        let snapshot = env.live_scopes_snapshot();
        assert_eq!(snapshot.len(), 2);
    }

    #[test]
    fn test_capture_env_deep_copies() {
        let mut env = Environment::new();
        env.define("x".to_string(), Value::Number(1.0));
        let captured = env.capture_env();
        assert_eq!(captured.get("x"), Some(Value::Number(1.0)));
    }

    #[test]
    fn test_binding_scope_finds_correct_scope() {
        let mut env = Environment::new();
        env.define("outer".to_string(), Value::Number(1.0));
        env.push_scope();
        env.define("inner".to_string(), Value::Number(2.0));
        let inner_scope = env.binding_scope("inner").unwrap();
        let outer_scope = env.binding_scope("outer").unwrap();
        assert!(!Rc::ptr_eq(&inner_scope, &outer_scope));
    }

    #[test]
    fn test_binding_scope_missing() {
        let env = Environment::new();
        assert!(env.binding_scope("nonexistent").is_none());
    }

    #[test]
    fn test_keys_returns_current_scope_names() {
        let mut env = Environment::new();
        env.define("a".to_string(), Value::Number(1.0));
        env.define("b".to_string(), Value::Number(2.0));
        let keys = env.keys();
        assert!(keys.contains(&"a".to_string()));
        assert!(keys.contains(&"b".to_string()));
    }

    #[test]
    fn test_set_parent_chain() {
        let parent = Rc::new(RefCell::new(Environment::new()));
        parent
            .borrow_mut()
            .define("p".to_string(), Value::Number(1.0));
        let child = Environment::with_parent(Rc::clone(&parent));
        assert_eq!(child.get("p"), Some(Value::Number(1.0)));
    }

    #[test]
    fn test_super_class_set_and_get() {
        let mut env = Environment::new();
        assert!(env.get_super_class().is_none());
        let obj = Rc::new(RefCell::new(crate::value::Object::new(
            crate::value::kind::ObjectKind::Ordinary,
        )));
        let obj_val = Value::Object(Rc::clone(&obj));
        env.set_super_class(obj_val);
        let result = env.get_super_class();
        assert!(result.is_some());
        // Compare by reference identity since Value::Object PartialEq returns false
        let Value::Object(result_rc) = result.unwrap() else {
            panic!("expected Object")
        };
        assert!(Rc::ptr_eq(&result_rc, &obj));
    }

    #[test]
    fn test_pending_fields_take() {
        let mut env = Environment::new();
        assert!(env.take_pending_fields().is_none());
        env.set_pending_fields(vec![]);
        assert!(env.take_pending_fields().is_some());
        assert!(env.take_pending_fields().is_none());
    }

    #[test]
    fn test_get_shared_returns_same_rc() {
        let mut env = Environment::new();
        env.define("shared".to_string(), Value::Number(42.0));
        let rc1 = env.get_shared("shared");
        let rc2 = env.get_shared("shared");
        assert!(rc1.is_some());
        assert!(rc2.is_some());
        assert!(Rc::ptr_eq(rc1.as_ref().unwrap(), rc2.as_ref().unwrap()));
    }
}
