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
