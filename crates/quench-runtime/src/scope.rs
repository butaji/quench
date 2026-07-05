//! Scope - a single frame in the environment's scope chain

use std::collections::HashMap;
use std::rc::Rc;
use crate::value::Value;
use crate::ast::VarKind;

/// Whether a variable was declared (hoisting support) but not yet initialized
#[derive(Debug, Clone, PartialEq)]
pub enum VarState {
    /// Variable is declared with a value (may be undefined)
    Initialized(Value),
    /// Variable was declared with `var` but initialization hasn't been evaluated yet
    /// This simulates the "hoisted but undefined" state of var declarations
    DeclaredOnly,
    /// Variable is in the Temporal Dead Zone (TDZ) - declared but not yet initialized
    /// This is used for let/const declarations
    TDZ,
}

/// An environment frame that holds variable bindings
#[derive(Debug, Clone)]
pub struct Scope {
    pub bindings: HashMap<String, Value>,
    /// Track var kinds for const enforcement and hoisting
    pub var_kinds: HashMap<String, VarKind>,
    /// Track variables declared with `var` that are in "declared only" state
    pub var_declarations: HashMap<String, VarState>,
    pub this_value: Option<Value>,
}

impl Scope {
    pub fn new() -> Self {
        Scope {
            bindings: HashMap::new(),
            var_kinds: HashMap::new(),
            var_declarations: HashMap::new(),
            this_value: None,
        }
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        if let Some(state) = self.var_declarations.get(name) {
            if let VarState::DeclaredOnly = state {
                return Some(Value::Undefined);
            }
        }
        self.bindings.get(name).cloned()
    }

    /// Get a reference-counted value (for interpreter use)
    pub fn get_rc(&self, name: &str) -> Option<Rc<Value>> {
        self.get(name).map(|v| Rc::new(v))
    }

    /// Check if a variable is in TDZ state (declared but not initialized)
    pub fn is_tdz(&self, name: &str) -> bool {
        matches!(self.var_declarations.get(name), Some(VarState::TDZ))
    }

    /// Check if a variable is in var_declarations (for TDZ or hoisted var)
    pub fn is_in_declarations(&self, name: &str) -> bool {
        self.var_declarations.contains_key(name)
    }

    /// Mark a variable as in TDZ (for let/const declarations)
    pub fn mark_tdz(&mut self, name: String) {
        self.var_declarations.insert(name, VarState::TDZ);
    }

    pub fn set(&mut self, name: String, value: Value) -> bool {
        match self.bindings.entry(name) {
            std::collections::hash_map::Entry::Occupied(mut e) => {
                e.insert(value);
                true
            }
            std::collections::hash_map::Entry::Vacant(_) => false,
        }
    }

    pub fn define(&mut self, name: String, value: Value) {
        self.var_declarations.remove(&name);
        self.bindings.insert(name, value);
    }

    /// Define a variable with its kind (for const enforcement)
    pub fn define_with_kind(&mut self, name: String, value: Value, kind: VarKind) {
        self.bindings.insert(name.clone(), value);
        self.var_kinds.insert(name, kind);
    }

    /// Check if a variable exists (including declared-only vars)
    pub fn has(&self, name: &str) -> bool {
        self.bindings.contains_key(name) || self.var_declarations.contains_key(name)
    }

    /// Get the kind of a variable
    pub fn get_kind(&self, name: &str) -> Option<VarKind> {
        self.var_kinds.get(name).copied()
    }

    /// Mark a variable as "declared only" (for var hoisting)
    pub fn declare_var(&mut self, name: String, kind: VarKind) {
        match kind {
            VarKind::Var => {
                self.var_declarations.insert(name.clone(), VarState::DeclaredOnly);
            }
            VarKind::Let | VarKind::Const => {
                self.var_declarations.insert(name.clone(), VarState::TDZ);
            }
        }
        self.var_kinds.insert(name, kind);
    }

    /// Initialize a declared-only var
    pub fn initialize_declared(&mut self, name: &str, value: Value) {
        self.var_declarations.remove(name);
        self.bindings.insert(name.to_string(), value);
    }

    /// Check if a variable is declared but not yet initialized
    pub fn is_declared_only(&self, name: &str) -> bool {
        match self.var_declarations.get(name) {
            Some(VarState::DeclaredOnly) | Some(VarState::TDZ) => true,
            _ => false,
        }
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
