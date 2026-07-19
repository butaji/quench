//! Environment (scope chain) for the JavaScript interpreter

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{Expression, PropertyKey, VarKind};
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

    pub fn set_object_property(&mut self, name: &str, value: Value, strict: bool) -> Option<bool> {
        let object = self.object_binding.as_ref()?;
        if !self.bindings.contains_key(name) {
            return None;
        }
        if !object.borrow().has(name) {
            return Some(!strict);
        }
        // Check writability: non-writable → strict throws, sloppy returns Ok.
        if let Some(flags) = object.borrow().get_descriptor(name) {
            if !flags.writable {
                return None; // signals caller to fall through to assign_to_identifier
            }
        }
        object.borrow_mut().set(name, value.clone());
        self.bindings.insert(name.to_string(), Rc::new(value));
        Some(true)
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

    pub fn set(&mut self, name: String, value: Value, strict: bool) -> bool {
        // Per ES §13.15.2: assigning to a const binding throws TypeError
        if matches!(self.var_kinds.get(&name), Some(VarKind::Const)) {
            return false; // Caller will throw TypeError
        }
        match self.bindings.entry(name.clone()) {
            std::collections::hash_map::Entry::Occupied(mut e) => {
                // Strict mode: assignment to non-writable global property (NaN/Infinity/
                // undefined) must throw TypeError. Check via object_binding descriptor.
                if strict {
                    if let Some(ref obj) = self.object_binding {
                        if let Some(flags) = obj.borrow().get_descriptor(&name) {
                            if !flags.writable {
                                return false; // Caller will throw TypeError
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

    /// Get the "this" binding for this scope
    pub fn get_this(&self) -> Option<Value> {
        self.this_value.clone()
    }

    /// Set the "this" binding for this scope
    pub fn set_this(&mut self, value: Value) {
        self.this_value = Some(value);
        self.this_initialized = true;
    }

    /// Bind `this` without marking it as initialized. Used by build_constructor_env
    /// before super() runs — the flag is set later by mark_this_initialized().
    pub fn set_this_value(&mut self, value: Value) {
        self.this_value = Some(value);
    }

    /// Mark `this` as initialized. Called after super() succeeds in a derived
    /// class constructor, so a subsequent super() throws ReferenceError.
    pub fn mark_this_initialized(&mut self) {
        self.this_initialized = true;
    }

    /// Check whether `this` has been initialized in this scope. Used to
    /// reject duplicate `super()` / this re-binding attempts per ES §12.3.5.1.
    pub fn is_this_initialized(&self) -> bool {
        self.this_initialized
    }
}

impl Default for Scope {
    fn default() -> Self {
        Self::new()
    }
}

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

/// Helper: is the named own property writable on a ValueFunction?
/// Per ES §9.2.4 FunctionInitialize, `length` and `name` are non-writable.
fn is_writable_function_prop(key: &str) -> bool {
    !matches!(key, "length" | "name")
}

impl Environment {
    /// Create a new top-level environment
    pub fn new() -> Self {
        Environment {
            scopes: vec![Rc::new(RefCell::new(Scope::new()))],
            parent: None,
            super_class: None,
            pending_fields: None,
        }
    }

    /// Create a new environment with a parent
    pub fn with_parent(parent: Rc<RefCell<Environment>>) -> Self {
        Environment {
            scopes: vec![Rc::new(RefCell::new(Scope::new()))],
            parent: Some(parent),
            super_class: None,
            pending_fields: None,
        }
    }

    /// Attach a parent environment. Used by closure-capture code to
    /// link the new capture env to the live outer chain.
    pub fn set_parent(&mut self, parent: Rc<RefCell<Environment>>) {
        self.parent = Some(parent);
    }

    /// Set the super class reference for class methods/constructors
    pub fn set_super_class(&mut self, super_class: Value) {
        self.super_class = Some(super_class);
    }

    /// Get the super class reference
    pub fn get_super_class(&self) -> Option<Value> {
        self.super_class.clone()
    }

    /// Set pending field initializers (for derived classes with fields).
    /// These are evaluated after super() returns, per ES §13.2.6.1.
    pub fn set_pending_fields(&mut self, fields: Vec<(PropertyKey, Expression)>) {
        self.pending_fields = Some(fields);
    }

    /// Take pending field initializers, if any (consumes them so they
    /// only run once).
    pub fn take_pending_fields(&mut self) -> Option<Vec<(PropertyKey, Expression)>> {
        self.pending_fields.take()
    }

    /// Get the parent environment
    pub fn get_parent(&self) -> Option<Rc<RefCell<Environment>>> {
        self.parent.clone()
    }

    /// Iterate live scopes in stack order (outermost first, innermost last).
    /// "Live" means still in `self.scopes` (popped scopes are removed).
    /// Callers should not retain the iterator past a mutation of `self.scopes`.
    fn live_scopes(&self) -> impl Iterator<Item = &Rc<RefCell<Scope>>> {
        self.scopes.iter()
    }

    /// Snapshot the live scope chain as a `Vec<Rc<RefCell<Scope>>>`.
    /// The returned `Rc`s are SHARED with this environment, so closures
    /// (and getters/setters, class methods, …) created from this snapshot
    /// see the same bindings as the active execution — writes propagate
    /// and later initialization is visible. The closure env keeps these
    /// `Rc`s alive even after the block pops, so the closure can
    /// continue to reach its captured bindings.
    pub fn live_scopes_snapshot(&self) -> Vec<Rc<RefCell<Scope>>> {
        self.scopes.iter().cloned().collect()
    }

    /// Build a fresh `Environment` that owns a snapshot of the live
    /// scope chain AND inherits the active environment's `parent`. Used
    /// by every closure-capture site (function, arrow, getter/setter,
    /// class method, function declaration in a block) so they all share
    /// the same lexical Environment Records.
    pub fn capture_env(&self) -> Environment {
        let mut captured = Environment::new();
        captured.scopes = self.live_scopes_snapshot();
        captured.parent = self.parent.clone();
        captured.super_class = self.super_class.clone();
        // pending_fields intentionally NOT captured — only valid in the
        // constructor's own environment during instantiation.
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

    /// Get a variable by name (lexical lookup)
    /// Returns a cloned Value for simplicity.
    /// Walks the scope chain in this environment and then the parent
    /// chain. Falls back to globalThis at the top level.
    pub fn get(&self, name: &str) -> Option<Value> {
        for scope_rc in self.scopes.iter().rev() {
            if let Some(value) = scope_rc.borrow().get(name) {
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
        for scope_rc in self.scopes.iter().rev() {
            if let Some(rc) = scope_rc.borrow().get_rc(name) {
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
    fn get_global_this_property(&self, name: &str) -> Option<Value> {
        for scope_rc in self.scopes.iter() {
            let scope = scope_rc.borrow();
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
    /// For arrow functions, attempting to set `caller` or `arguments` is a
    /// silent no-op (the actual TypeError is thrown by the assignment path).
    /// For non-writable own properties (e.g. `length` on a function), the set
    /// is silently ignored in sloppy mode; the strict path falls through to
    /// assign_to_member which throws TypeError.
    pub fn set_property(&mut self, name: &str, prop: &str, value: Value) -> bool {
        for scope_rc in self.scopes.iter().rev() {
            let mut scope = scope_rc.borrow_mut();
            if let std::collections::hash_map::Entry::Occupied(entry) =
                scope.bindings.entry(name.to_string())
            {
                let rc = entry.get();
                match rc.as_ref() {
                    Value::Function(ref f) => {
                        if f.is_arrow && (prop == "caller" || prop == "arguments") {
                            return false;
                        }
                        // Per ES §10.2.9, [[Set]] on a non-writable own
                        // property returns silently in sloppy mode. In
                        // strict mode the assignment path raises TypeError.
                        if !crate::interpreter::is_strict_mode()
                            && f.get_property(prop).is_some()
                            && !is_writable_function_prop(prop)
                        {
                            return true; // silently ignored, no-op
                        }
                        if crate::interpreter::is_strict_mode()
                            && f.get_property(prop).is_some()
                            && !is_writable_function_prop(prop)
                        {
                            return false; // let assign_to_member throw
                        }
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
        // Try to set in parent
        if let Some(ref parent) = self.parent {
            return parent.borrow_mut().set(name, value);
        }
        false
    }

    /// Define a new variable in the current (innermost) scope.
    pub fn define(&mut self, name: String, value: Value) {
        if let Some(mut scope) = self.current_scope_ref_mut() {
            scope.define(name, value);
        }
    }

    /// Declare a variable (same as define, for compatibility)
    pub fn declare(&mut self, name: String, value: Value) {
        self.define(name, value);
    }

    /// Declare a variable with its kind (for var/let/const handling).
    pub fn declare_var(&mut self, name: String, kind: VarKind) {
        if let Some(mut scope) = self.current_scope_ref_mut() {
            scope.declare_var(name, kind);
        }
    }

    /// Initialize a declared variable (removes from declarations, adds
    /// to bindings). Finds the innermost scope where the variable was
    /// declared; if no pending declaration exists, updates the existing
    /// local binding instead of letting the write escape to the parent
    /// environment.
    pub fn initialize_declared(&mut self, name: &str, value: Value) {
        // Search from innermost scope outward so block-scoped declarations
        // shadow outer declarations, matching JavaScript lexical scoping.
        for scope_rc in self.scopes.iter().rev() {
            let mut scope = scope_rc.borrow_mut();
            if scope.declarations.contains_key(name) {
                scope.initialize_declared(name, value);
                return;
            }
        }

        // No pending declaration: update the existing local binding in
        // place (e.g. a `var` re-initialized on a later loop iteration).
        for scope_rc in self.scopes.iter().rev() {
            let mut scope = scope_rc.borrow_mut();
            if scope.bindings.contains_key(name) {
                scope.set(
                    name.to_string(),
                    value,
                    crate::interpreter::is_strict_mode(),
                );
                return;
            }
        }

        // Otherwise delegate to the parent environment.
        if let Some(ref parent) = self.parent {
            parent.borrow_mut().initialize_declared(name, value);
        }
    }

    /// Check if a variable is in TDZ in the current scope.
    pub fn is_tdz(&self, name: &str) -> bool {
        if let Some(scope) = self.current_scope_ref() {
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
        for scope_rc in self.scopes.iter().rev() {
            if let Some(kind) = scope_rc.borrow().get_kind(name) {
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
        for scope_rc in self.scopes.iter() {
            if scope_rc.borrow().has(name) {
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

    /// Push a new scope onto the live stack. The scope is also retained in
    /// `scopes` so closures that captured the surrounding chain keep
    /// working even after it is popped.
    pub fn push_scope(&mut self) {
        self.scopes.push(Rc::new(RefCell::new(Scope::new())));
    }

    /// Pop the current (top) scope. The underlying `Rc<RefCell<Scope>>`
    /// stays alive as long as any captured snapshot in a closure env
    /// still references it, so closures created inside the block keep
    /// seeing its bindings even though this environment no longer does.
    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    /// Get the current (innermost) scope. The returned `Rc` shares the
    /// underlying `Scope` with any other Environment that captured the
    /// same logical scope.
    pub fn current_scope(&self) -> Rc<RefCell<Scope>> {
        Rc::clone(
            self.scopes
                .last()
                .expect("environment always has at least one scope"),
        )
    }

    /// Convenience for callers that need a `&Scope` view without holding
    /// the `Rc` themselves.
    fn current_scope_ref(&self) -> Option<std::cell::Ref<'_, Scope>> {
        self.scopes.last().map(|s| s.borrow())
    }

    /// Convenience for callers that need a `&mut Scope` view.
    fn current_scope_ref_mut(&self) -> Option<std::cell::RefMut<'_, Scope>> {
        self.scopes.last().map(|s| s.borrow_mut())
    }

    /// Get all variable names in the current scope
    pub fn keys(&self) -> Vec<String> {
        self.current_scope_ref()
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
        // code that clones an environment. Scope entries are shared via
        // Rc, so captured closures see the same lexical records.
        // pending_fields is NOT cloned — only valid for the constructor's
        // own environment during instantiation.
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
