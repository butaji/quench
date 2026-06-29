//! Environment (scope chain) for the JavaScript interpreter

use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;

use crate::value::Value;

/// An environment frame that holds variable bindings
#[derive(Debug, Clone)]
pub struct Scope {
    bindings: HashMap<String, Value>,
    this_value: Option<Value>,
}

impl Scope {
    pub fn new() -> Self {
        Scope {
            bindings: HashMap::new(),
            this_value: None,
        }
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        self.bindings.get(name).cloned()
    }

    /// Get a reference-counted value (for interpreter use)
    pub fn get_rc(&self, name: &str) -> Option<Rc<Value>> {
        self.bindings.get(name).map(|v| Rc::new(v.clone()))
    }

    pub fn set(&mut self, name: String, value: Value) -> bool {
        if self.bindings.contains_key(&name) {
            self.bindings.insert(name, value);
            true
        } else {
            false
        }
    }

    pub fn define(&mut self, name: String, value: Value) {
        self.bindings.insert(name, value);
    }

    pub fn has(&self, name: &str) -> bool {
        self.bindings.contains_key(name)
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
#[derive(Debug, Clone)]
pub struct Environment {
    pub scopes: Vec<Scope>,
}

impl Environment {
    /// Create a new top-level environment
    pub fn new() -> Self {
        Environment {
            scopes: vec![Scope::new()],
        }
    }

    /// Create a new environment with a parent
    pub fn with_parent(parent: Rc<RefCell<Environment>>) -> Self {
        let mut env = Environment {
            scopes: vec![Scope::new()],
        };
        // Link to parent by storing it specially
        // For simplicity, we'll copy parent bindings into current scope
        let parent_env = parent.borrow();
        for scope in &parent_env.scopes {
            for (name, value) in &scope.bindings {
                env.scopes.last_mut().unwrap().define(name.clone(), value.clone());
            }
        }
        env
    }

    /// Get a variable by name (lexical lookup)
    /// Returns a cloned Value for simplicity.
    pub fn get(&self, name: &str) -> Option<Value> {
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return Some(value);
            }
        }
        None
    }
    
    /// Get a variable by name, returning an Rc for identity preservation.
    /// For function values, this ensures the same closure is used.
    pub fn get_rc(&self, name: &str) -> Option<Rc<Value>> {
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return Some(Rc::new(value));
            }
        }
        None
    }

    /// Set a variable by name (assigns to existing binding)
    pub fn set(&mut self, name: &str, value: Value) -> bool {
        for scope in self.scopes.iter_mut().rev() {
            if scope.set(name.to_string(), value.clone()) {
                return true;
            }
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

    /// Check if a variable exists in any scope
    pub fn has(&self, name: &str) -> bool {
        for scope in &self.scopes {
            if scope.has(name) {
                return true;
            }
        }
        false
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
        self.scopes.last().map(|s| s.bindings.keys().cloned().collect()).unwrap_or_default()
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self::new()
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
