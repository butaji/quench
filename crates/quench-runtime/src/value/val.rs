//! JavaScript runtime values - the core runtime type.
//!
//! A JavaScript value - the fundamental runtime type.
//! All values are immutable handles; objects are Rc<RefCell<Object>> for mutation.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::env::Environment;
use crate::JsError;

use num_bigint::BigInt;

use crate::ast::{Class, ClassMember, Param, PropertyKey};
use crate::eval;
use crate::value::function::{NativeConstructor, NativeFunction, ValueFunction};
use crate::value::object::Object;

/// Internal object key for a private name — distinct from public `"#name"` string keys.
pub fn private_name_key(name: &str) -> String {
    let bare = name.strip_prefix('#').unwrap_or(name);
    format!("\0private:{bare}\0")
}

/// Class-scoped private name storage key.
pub fn scoped_private_name_key(class_id: usize, name: &str) -> String {
    let bare = name
        .strip_prefix('#')
        .unwrap_or(name)
        .trim_start_matches("\0private:")
        .trim_end_matches('\0');
    format!("\0private:{class_id}:{bare}\0")
}

/// Whether `key` is a private name storage slot or `#`-prefixed source ident.
pub fn is_private_element_key(key: &str) -> bool {
    key.starts_with('#') || is_private_name_key(key)
}

/// Whether `key` is a [`private_name_key`] storage slot.
pub fn is_private_name_key(key: &str) -> bool {
    key.starts_with("\0private:") && key.ends_with('\0')
}

/// Unscoped private name from the parser (before class id assignment).
pub fn is_unscoped_private_name_key(key: &str) -> bool {
    if !is_private_name_key(key) {
        return false;
    }
    let inner = &key[9..key.len() - 1];
    !inner.contains(':')
}

/// A class method: (name, params, body, is_async, is_generator)
pub type ClassMethod = (
    PropertyKey,
    Vec<Param>,
    Vec<crate::ast::Statement>,
    bool,
    bool,
);

#[allow(unused_imports)] // Re-exported for external use
pub use crate::value::convert::{
    loose_eq, strict_eq, to_bool, to_js_string, to_number, to_primitive,
};

/// ECMA-262 6.1.6 Symbol type — unique, immutable, optionally described.
/// Property keys use [`Symbol::property_key`] (`desc\0id`) so equal
/// descriptions never collide.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Symbol {
    pub desc: Option<Rc<str>>,
    pub global: bool, // Symbol.for / Symbol.keyFor
    pub id: u64,
}

static SYMBOL_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl Symbol {
    pub fn new(desc: Option<Rc<str>>, global: bool) -> Self {
        Self {
            desc,
            global,
            id: SYMBOL_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        }
    }

    /// Canonical property-key form: `desc\0id` (AGENTS.md).
    pub fn property_key(&self) -> String {
        format!("{}\0{}", self.desc.as_deref().unwrap_or(""), self.id)
    }
}

/// A JavaScript value - the fundamental runtime type.
/// All values are immutable handles; objects are Rc<RefCell<Object>> for mutation.
#[derive(Clone)]
pub enum Value {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    /// Objects are reference-counted with interior mutability
    Object(Rc<RefCell<Object>>),
    /// Functions hold their closure environment and have cached prototypes
    Function(ValueFunction),
    /// Native functions (host functions) are Arc-wrapped closures
    NativeFunction(Rc<NativeFunction>),
    /// Native constructors (Date, Error, etc.) - have a prototype property
    NativeConstructor(Rc<NativeConstructor>),
    /// Symbols for unique property keys (TComp: Rc<Symbol> per spec 6.1.6)
    Symbol(Rc<Symbol>),
    /// ES6 class - callable constructor with prototype chain
    Class(Box<ClassValue>),
    /// BigInt arbitrary-precision integer
    BigInt(Rc<BigInt>),
    /// Generator object — returned by generator functions (function*)
    Generator(Rc<RefCell<crate::value::generator::GeneratorObject>>),
}

/// Global counter for unique class identity (used for === comparison)
static CLASS_ID_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// ES6 class representation
/// Holds the class definition and provides methods to create instances
#[derive(Debug, Clone)]
pub struct ClassValue {
    /// Unique identity assigned at construction, preserved across clones.
    /// Value::clone deep-copies ClassValue, so this is the only stable
    /// identity for `C === C` comparisons.
    pub(crate) id: usize,
    /// Class name (optional, for named class expressions)
    pub name: Option<String>,
    /// Constructor parameters
    pub constructor_params: Vec<String>,
    /// Constructor body statements
    pub constructor_body: Vec<crate::ast::Statement>,
    /// True when the class AST had an explicit `constructor` member.
    /// Missing constructor on a derived class gets a synthetic
    /// `constructor(...args){ super(...args); }` — only then may we auto-call super.
    pub has_explicit_constructor: bool,
    /// Instance methods
    pub methods: Vec<ClassMethod>,
    /// Static methods
    pub static_methods: Vec<ClassMethod>,
    /// Instance getters (name -> body)
    pub getters: Vec<(PropertyKey, Vec<crate::ast::Statement>)>,
    /// Static getters (name -> body)
    pub static_getters: Vec<(PropertyKey, Vec<crate::ast::Statement>)>,
    /// Instance setters (name -> (param, body))
    pub setters: Vec<(PropertyKey, String, Vec<crate::ast::Statement>)>,
    /// Static setters (name -> (param, body))
    pub static_setters: Vec<(PropertyKey, String, Vec<crate::ast::Statement>)>,
    /// Instance fields (name -> value expression)
    pub instance_fields: Vec<(PropertyKey, crate::ast::Expression)>,
    /// Static fields (name -> value expression)
    pub static_fields: Vec<(PropertyKey, crate::ast::Expression)>,
    /// Static initialization blocks (ES2022): static { body }
    pub static_blocks: Vec<Vec<crate::ast::Statement>>,
    /// Original ordered class members for sequential evaluation
    pub ordered_members: Vec<crate::ast::ClassMember>,
    /// Superclass expression (None for no extends)
    pub(crate) super_class: Option<Box<crate::ast::Expression>>,
    /// The class constructor's own `[[Prototype]]` (the superclass constructor).
    /// Used by Object.getPrototypeOf(class) to return the superclass, not
    /// C.prototype (which is stored in prototype_cell). For `extends null`,
    /// this is None, meaning Object.getPrototypeOf(C) returns null.
    /// Uses Rc so all clones of ClassValue share the same value.
    /// Stores `Value` directly: `Value::Class(super_class)` for `extends Base`,
    /// or `Value::Object(Function.prototype)` for no-extends.
    pub(crate) super_class_own_proto_cell: std::rc::Rc<std::cell::RefCell<Option<Value>>>,
    /// Cached prototype object for instanceof checks (C.prototype).
    /// Uses Rc so all clones of ClassValue share the same cache.
    pub(crate) prototype_cell:
        std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<std::cell::RefCell<Object>>>>>,
    /// Static field values (name -> value), initialized during class expression evaluation
    pub(crate) static_properties_cell: std::rc::Rc<std::cell::RefCell<HashMap<String, Value>>>,
    /// Deleted property names (configurable properties like "name" that were deleted)
    pub(crate) deleted_properties:
        std::rc::Rc<std::cell::RefCell<std::collections::HashSet<String>>>,
    /// Class definition environment - used to evaluate computed property keys
    /// for static accessors with the correct lexical scope.
    pub(crate) class_def_env_cell:
        std::rc::Rc<std::cell::RefCell<Option<Rc<RefCell<Environment>>>>>,
    /// Keys resolved during class definition for static getters/setters (by index).
    pub(crate) static_getter_keys_cell: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    pub(crate) static_setter_keys_cell: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    /// Whether new private static fields may be added (Object.preventExtensions).
    pub(crate) extensible_cell: std::rc::Rc<std::cell::RefCell<bool>>,
}

impl ClassValue {
    /// Create a ClassValue from an AST Class node
    #[allow(dead_code)]
    pub fn from_ast(class: &Class) -> Self {
        let id = CLASS_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut scoped_class = class.clone();
        crate::eval::class::private_names::scope_class_private_names(&mut scoped_class, id);

        let mut constructor_params = Vec::new();
        let mut constructor_body = Vec::new();
        let mut has_explicit_constructor = false;
        let mut methods = Vec::new();
        let mut static_methods = Vec::new();
        let mut getters = Vec::new();
        let mut static_getters = Vec::new();
        let mut setters = Vec::new();
        let mut static_setters = Vec::new();
        let mut instance_fields = Vec::new();
        let mut static_fields = Vec::new();
        let mut static_blocks = Vec::new();

        fill_members_from_ast(
            &scoped_class.body,
            &mut constructor_params,
            &mut constructor_body,
            &mut has_explicit_constructor,
            &mut methods,
            &mut static_methods,
            &mut getters,
            &mut static_getters,
            &mut setters,
            &mut static_setters,
            &mut instance_fields,
            &mut static_fields,
            &mut static_blocks,
        );

        ClassValue {
            id,
            name: scoped_class.name.clone(),
            constructor_params,
            constructor_body,
            has_explicit_constructor,
            methods,
            static_methods,
            getters,
            static_getters,
            setters,
            static_setters,
            instance_fields,
            static_fields,
            static_blocks,
            ordered_members: scoped_class.body.clone(),
            super_class: scoped_class.super_class.clone(),
            super_class_own_proto_cell: std::rc::Rc::new(std::cell::RefCell::new(None::<Value>)),
            prototype_cell: std::rc::Rc::new(std::cell::RefCell::new(None)),
            static_properties_cell: std::rc::Rc::new(std::cell::RefCell::new(HashMap::new())),
            deleted_properties: std::rc::Rc::new(std::cell::RefCell::new(
                std::collections::HashSet::new(),
            )),
            class_def_env_cell: std::rc::Rc::new(std::cell::RefCell::new(None)),
            static_getter_keys_cell: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
            static_setter_keys_cell: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
            extensible_cell: std::rc::Rc::new(std::cell::RefCell::new(true)),
        }
    }

    pub fn push_static_getter_key(&self, key: String) {
        self.static_getter_keys_cell.borrow_mut().push(key);
    }

    pub fn push_static_setter_key(&self, key: String) {
        self.static_setter_keys_cell.borrow_mut().push(key);
    }

    pub fn static_getter_key(&self, index: usize) -> Option<String> {
        self.static_getter_keys_cell.borrow().get(index).cloned()
    }

    pub fn static_setter_key(&self, index: usize) -> Option<String> {
        self.static_setter_keys_cell.borrow().get(index).cloned()
    }

    /// Set the class definition environment (used for evaluating computed property keys in static accessors)
    pub fn set_class_def_env(&self, env: Rc<RefCell<Environment>>) {
        let mut cell = self.class_def_env_cell.borrow_mut();
        *cell = Some(env);
    }

    /// Get the class definition environment
    pub fn get_class_def_env(&self) -> Option<Rc<RefCell<Environment>>> {
        self.class_def_env_cell.borrow().clone()
    }

    /// Set the cached prototype for this class (shared across all clones)
    pub fn set_prototype(&self, proto: std::rc::Rc<std::cell::RefCell<Object>>) {
        let mut cell = self.prototype_cell.borrow_mut();
        *cell = Some(proto);
    }

    /// Set the class constructor's own `[[Prototype]]` (the superclass constructor).
    /// For `extends null`, this should be set to None so that
    /// Object.getPrototypeOf(class) returns null.
    pub fn set_super_class_own_proto(&self, proto: Option<Value>) {
        let mut cell = self.super_class_own_proto_cell.borrow_mut();
        *cell = proto;
    }

    /// Set a static field value on this class
    pub fn set_static_field(&self, name: &str, value: Value) -> Result<(), JsError> {
        if is_private_name_key(name) && !*self.extensible_cell.borrow() {
            let (_, js_err) = crate::value::error::create_js_error_with_type(
                "Cannot add private field to non-extensible object",
                "TypeError",
            );
            return Err(js_err);
        }
        self.static_properties_cell
            .borrow_mut()
            .insert(name.to_string(), value);
        Ok(())
    }

    pub fn set_extensible(&self, extensible: bool) {
        *self.extensible_cell.borrow_mut() = extensible;
    }

    pub fn is_extensible(&self) -> bool {
        *self.extensible_cell.borrow()
    }

    /// Get a static field value from this class
    pub fn get_static_field(&self, name: &str) -> Option<Value> {
        self.static_properties_cell.borrow().get(name).cloned()
    }

    /// Check if this class has a static setter for the given property name.
    pub fn has_static_setter(&self, name: &str) -> bool {
        let eval_env = self.class_def_env_cell.borrow();
        let env = match eval_env.as_ref() {
            Some(e) => e,
            None => return false,
        };
        for (i, (key, _param, _body)) in self.static_setters.iter().enumerate() {
            let key_str = if let Some(cached) = self.static_setter_key(i) {
                Ok(cached)
            } else {
                crate::eval::class::helpers::prop_key_to_string(key, env, false)
            };
            if key_str.is_ok_and(|k| k == name) {
                return true;
            }
        }
        false
    }

    /// Whether `name` is an own property of this class constructor (static members).
    pub fn has_static_own_property(&self, name: &str) -> bool {
        if self.deleted_properties.borrow().contains(name) {
            return false;
        }
        if name == "name" || name == "prototype" {
            return true;
        }
        if self.get_static_field(name).is_some() {
            return true;
        }
        let eval_env = self
            .get_class_def_env()
            .unwrap_or_else(|| Rc::new(RefCell::new(Environment::new())));
        for (key, _) in &self.static_getters {
            if crate::eval::class::helpers::prop_key_to_string(key, &eval_env, false)
                .is_ok_and(|k| !k.starts_with('#') && k == name)
            {
                return true;
            }
        }
        for (key, _, _) in &self.static_setters {
            if crate::eval::class::helpers::prop_key_to_string(key, &eval_env, false)
                .is_ok_and(|k| !k.starts_with('#') && k == name)
            {
                return true;
            }
        }
        for (key, _, _, _, _) in &self.static_methods {
            if crate::eval::class::helpers::prop_key_to_string(key, &eval_env, false)
                .is_ok_and(|k| !k.starts_with('#') && k == name)
            {
                return true;
            }
        }
        false
    }

    /// Set a static property on this class, invoking a setter if one exists.
    pub fn set_static_property(
        &self,
        name: &str,
        value: Value,
        env: &Rc<RefCell<Environment>>,
    ) -> Result<(), JsError> {
        // Check if there's a static setter
        let eval_env = self.class_def_env_cell.borrow();
        let env = eval_env
            .as_ref()
            .map(Rc::clone)
            .unwrap_or_else(|| Rc::clone(env));

        for (i, (key, param, body)) in self.static_setters.iter().enumerate() {
            let key_str = if let Some(cached) = self.static_setter_key(i) {
                cached
            } else {
                crate::eval::class::helpers::prop_key_to_string(key, &env, false)?
            };
            if key_str == name {
                // Bind `this` to the class itself so `this._v = v` in the setter body
                // writes to the class's own static properties, not a throwaway object.
                let this_val = Value::Class(Box::new(self.clone()));
                let mut call_env = Environment::with_parent(Rc::clone(&env));
                call_env.current_scope().borrow_mut().set_this(this_val);
                call_env.define(param.clone(), value);
                let call_env = Rc::new(RefCell::new(call_env));
                if !body.is_empty() {
                    let prev_strict = crate::interpreter::is_strict_mode();
                    crate::interpreter::set_strict_mode(false);
                    eval::statement::eval_function_body(body, &call_env, false)?;
                    // A setter's completion is discarded per ES §9.1.9 — consume a
                    // pending ControlFlow::Return so it can't leak into the next call.
                    let _ = crate::interpreter::take_control_flow();
                    crate::interpreter::set_strict_mode(prev_strict);
                }
                return Ok(());
            }
        }

        // No setter found, set the static field directly
        self.set_static_field(name, value)?;
        Ok(())
    }

    /// Set the inferred class name. Used so static field initializers can
    /// observe the eventual class name through `this.name` before the
    /// surrounding assignment has completed.
    pub fn set_name(&mut self, name: &str) {
        self.name = Some(name.to_string());
    }
}

/// Fill member vectors from AST class body.
#[allow(clippy::too_many_arguments)]
fn fill_members_from_ast(
    members: &[ClassMember],
    constructor_params: &mut Vec<String>,
    constructor_body: &mut Vec<crate::ast::Statement>,
    has_explicit_constructor: &mut bool,
    methods: &mut Vec<ClassMethod>,
    static_methods: &mut Vec<ClassMethod>,
    getters: &mut Vec<(PropertyKey, Vec<crate::ast::Statement>)>,
    static_getters: &mut Vec<(PropertyKey, Vec<crate::ast::Statement>)>,
    setters: &mut Vec<(PropertyKey, String, Vec<crate::ast::Statement>)>,
    static_setters: &mut Vec<(PropertyKey, String, Vec<crate::ast::Statement>)>,
    instance_fields: &mut Vec<(PropertyKey, crate::ast::Expression)>,
    static_fields: &mut Vec<(PropertyKey, crate::ast::Expression)>,
    static_blocks: &mut Vec<Vec<crate::ast::Statement>>,
) {
    for member in members {
        match member {
            ClassMember::Constructor { params, body } => {
                *constructor_params = params.clone();
                *constructor_body = body.clone();
                *has_explicit_constructor = true;
            }
            ClassMember::Method {
                name,
                params,
                body,
                is_async,
                is_generator,
            } => {
                methods.push((
                    name.clone(),
                    params.clone(),
                    body.clone(),
                    *is_async,
                    *is_generator,
                ));
            }
            ClassMember::StaticMethod {
                name,
                params,
                body,
                is_async,
                is_generator,
            } => {
                static_methods.push((
                    name.clone(),
                    params.clone(),
                    body.clone(),
                    *is_async,
                    *is_generator,
                ));
            }
            ClassMember::Getter { name, body } => {
                getters.push((name.clone(), body.clone()));
            }
            ClassMember::StaticGetter { name, body } => {
                static_getters.push((name.clone(), body.clone()));
            }
            ClassMember::Setter { name, param, body } => {
                setters.push((name.clone(), param.clone(), body.clone()));
            }
            ClassMember::StaticSetter { name, param, body } => {
                static_setters.push((name.clone(), param.clone(), body.clone()));
            }
            ClassMember::Field { name, value } => {
                instance_fields.push((name.clone(), value.clone()));
            }
            ClassMember::StaticField { name, value } => {
                static_fields.push((name.clone(), value.clone()));
            }
            ClassMember::StaticBlock { body } => {
                static_blocks.push(body.clone());
            }
        }
    }
}

impl Value {
    /// Check if this value is a Symbol whose description contains the given substring.
    pub fn is_symbol_with(&self, desc_contains: &str) -> bool {
        match self {
            Value::Symbol(s) => s.desc.as_ref().is_some_and(|d| d.contains(desc_contains)),
            _ => false,
        }
    }

    /// Check if this value is callable (a function).
    pub fn is_callable(&self) -> bool {
        matches!(
            self,
            Value::Function(_)
                | Value::NativeFunction(_)
                | Value::NativeConstructor(_)
                | Value::Class(_)
        )
    }

    /// Get a method by name from this value (for objects).
    /// Returns None if not an object or method doesn't exist.
    pub fn get_method(&self, name: &str) -> Option<Value> {
        match self {
            Value::Object(obj) => {
                let obj = obj.borrow();
                obj.get(name)
            }
            Value::Function(func) => func.get_property(name),
            _ => None,
        }
    }

    /// Get the prototype of a constructor/function value.
    /// Returns the prototype object if this is a constructor.
    pub fn get_prototype(&self) -> Option<Rc<RefCell<Object>>> {
        match self {
            Value::Object(o) => o.borrow().get("prototype").and_then(|p| {
                if let Value::Object(proto) = p {
                    Some(proto.clone())
                } else {
                    None
                }
            }),
            Value::Function(f) => Some(f.get_prototype()),
            Value::NativeFunction(nf) => nf.prototype.borrow().as_ref().map(Rc::clone),
            Value::NativeConstructor(nc) => Some(Rc::clone(&nc.prototype)),
            _ => None,
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Undefined, Value::Undefined) => true,
            (Value::Null, Value::Null) => true,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::BigInt(a), Value::BigInt(b)) => a == b,
            _ => false,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", to_js_string(self))
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        debug_value(self, f)
    }
}

fn debug_value(v: &Value, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if matches!(v, Value::Undefined | Value::Null | Value::Symbol(_)) {
        return debug_nullish_or_symbol(v, f);
    }
    match v {
        Value::Boolean(b) => write!(f, "{}", b),
        Value::Number(n) => write!(f, "{}", n),
        Value::String(s) => write!(f, "{:?}", s),
        Value::Object(_) => write!(f, "Object(...)"),
        Value::Function(_)
        | Value::NativeFunction(_)
        | Value::NativeConstructor(_)
        | Value::Generator(_)
        | Value::Class(_) => {
            write!(f, "Function(...)")
        }
        Value::BigInt(bi) => write!(f, "{}n", bi),
        _ => write!(f, "undefined"),
    }
}

fn debug_nullish_or_symbol(v: &Value, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match v {
        Value::Undefined => write!(f, "undefined"),
        Value::Null => write!(f, "null"),
        Value::Symbol(s) => write!(f, "Symbol({:?})", s.desc),
        _ => write!(f, "undefined"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ObjectKind;

    fn make_env() -> Rc<RefCell<crate::env::Environment>> {
        Rc::new(RefCell::new(crate::env::Environment::new()))
    }

    fn obj() -> Value {
        Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))))
    }

    fn sym(desc: &str) -> Value {
        Value::Symbol(Rc::new(Symbol::new(Some(Rc::from(desc)), false)))
    }

    fn big(n: i32) -> Value {
        Value::BigInt(Rc::new(BigInt::from(n)))
    }

    #[test]
    fn test_display_all_variants() {
        assert_eq!(format!("{}", Value::Undefined), "undefined");
        assert_eq!(format!("{}", Value::Null), "null");
        assert_eq!(format!("{}", Value::Boolean(true)), "true");
        assert_eq!(format!("{}", Value::Boolean(false)), "false");
        assert_eq!(format!("{}", Value::Number(42.0)), "42");
        assert_eq!(format!("{}", Value::Number(-0.0)), "0");
        assert_eq!(format!("{}", Value::String("hi".into())), "hi");
        assert_eq!(format!("{}", sym("test")), "Symbol(test)");
        assert_eq!(format!("{}", obj()), "[object Object]");
        let nf = Value::NativeFunction(Rc::new(NativeFunction::new(|_| Ok(Value::Undefined))));
        assert_eq!(format!("{}", nf), "[Function]");
        assert_eq!(format!("{}", big(123)), "123n");
    }

    #[test]
    fn test_debug_all_variants() {
        assert_eq!(format!("{:?}", Value::Undefined), "undefined");
        assert_eq!(format!("{:?}", Value::Null), "null");
        assert_eq!(format!("{:?}", Value::Boolean(true)), "true");
        assert_eq!(format!("{:?}", Value::Number(42.0)), "42");
        assert_eq!(format!("{:?}", Value::String("x".into())), "\"x\"");
        let env = make_env();
        let func = Value::Function(ValueFunction::new(None, vec![], vec![], env, false, false));
        assert_eq!(format!("{:?}", func), "Function(...)");
        assert_eq!(format!("{:?}", obj()), "Object(...)");
        assert_eq!(format!("{:?}", sym("x")), "Symbol(Some(\"x\"))");
        assert_eq!(format!("{:?}", big(42)), "42n");
        assert_eq!(format!("{:?}", big(-7)), "-7n");
    }

    #[test]
    fn test_clone_all_variants() {
        let cases = [
            Value::Undefined,
            Value::Null,
            Value::Boolean(true),
            Value::Number(10.5),
            Value::String("cl".into()),
            big(999),
        ];
        for v in &cases {
            assert_eq!(v.clone(), *v, "Clone failed for {0:?}", v);
        }
        // Object/Symbol: PartialEq always returns false, verify type match
        let o = obj();
        assert!(matches!(o.clone(), Value::Object(_)));
        let s = sym("k");
        assert!(matches!(s.clone(), Value::Symbol(_)));
    }

    #[test]
    fn test_partial_eq_same() {
        assert_eq!(Value::Undefined, Value::Undefined);
        assert_eq!(Value::Null, Value::Null);
        assert_eq!(Value::Boolean(true), Value::Boolean(true));
        assert_eq!(Value::Number(1.0), Value::Number(1.0));
        assert_eq!(Value::String("a".into()), Value::String("a".into()));
        assert_eq!(big(5), big(5));
    }

    #[test]
    fn test_partial_eq_different() {
        let all = [
            Value::Undefined,
            Value::Null,
            Value::Boolean(true),
            Value::Number(0.0),
            Value::String("".into()),
            big(0),
        ];
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                if i != j {
                    assert_ne!(*a, *b, "Expected {0:?} != {1:?}", a, b);
                }
            }
        }
        // Objects/Symbols are never equal via PartialEq
        assert_ne!(obj(), obj());
        assert_ne!(sym("k"), sym("k"));
    }

    #[test]
    fn test_symbol_struct() {
        let s = Symbol::new(Some(Rc::from("foo")), false);
        assert_eq!(s.desc.as_deref(), Some("foo"));
        assert!(!s.global);
        let s2 = Symbol::new(Some(Rc::from("foo")), false);
        assert_ne!(s.id, s2.id);
        assert_ne!(s.property_key(), s2.property_key());
        let sg = Symbol::new(None, true);
        assert!(sg.global);
        assert!(sg.desc.is_none());
    }

    #[test]
    fn test_bigint_handling() {
        assert!(matches!(big(42), Value::BigInt(_)));
        assert_eq!(big(42), big(42));
        assert_eq!(big(0), big(0));
        use std::str::FromStr;
        let large = BigInt::from_str("12345678901234567890").unwrap();
        assert_eq!(
            format!("{}", Value::BigInt(Rc::new(large))),
            "12345678901234567890n"
        );
    }

    #[test]
    fn test_is_callable_all() {
        let env = make_env();
        assert!(!Value::Undefined.is_callable());
        assert!(!Value::Null.is_callable());
        assert!(!Value::Boolean(true).is_callable());
        assert!(!Value::Number(1.0).is_callable());
        assert!(!Value::String("x".into()).is_callable());
        assert!(!obj().is_callable());
        assert!(!sym("x").is_callable());
        assert!(!big(1).is_callable());
        assert!(
            Value::Function(ValueFunction::new(None, vec![], vec![], env, false, false))
                .is_callable()
        );
        assert!(
            Value::NativeFunction(Rc::new(NativeFunction::new(|_| Ok(Value::Undefined))))
                .is_callable()
        );
        let proto = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
        assert!(Value::NativeConstructor(Rc::new(NativeConstructor::new(
            |_| Ok(Value::Undefined),
            proto,
        )))
        .is_callable());
    }

    #[test]
    fn test_class_has_static_setter() {
        use crate::ast::{Class, ClassMember, PropertyKey};
        // Build a class with a static setter
        let setter_member = ClassMember::StaticSetter {
            name: PropertyKey::String("foo".to_string()),
            param: "v".to_string(),
            body: vec![],
        };
        let class = Class {
            name: Some("C".to_string()),
            super_class: None,
            body: vec![setter_member],
        };
        let class_val = ClassValue::from_ast(&class);
        let env = make_env();
        class_val.set_class_def_env(Rc::clone(&env));

        // has_static_setter returns true for existing setter
        assert!(class_val.has_static_setter("foo"));
        // has_static_setter returns false for non-existing setter
        assert!(!class_val.has_static_setter("bar"));
    }

    #[test]
    fn test_class_set_static_property_with_setter() {
        use crate::ast::{Class, ClassMember, PropertyKey, Statement};
        // Build a class with a static setter that records the value
        let setter_member = ClassMember::StaticSetter {
            name: PropertyKey::String("foo".to_string()),
            param: "v".to_string(),
            body: vec![Statement::Empty],
        };
        let class = Class {
            name: Some("C".to_string()),
            super_class: None,
            body: vec![setter_member],
        };
        let class_val = ClassValue::from_ast(&class);
        let env = make_env();
        class_val.set_class_def_env(Rc::clone(&env));

        // set_static_property invokes the setter (doesn't throw)
        let result = class_val.set_static_property("foo", Value::Number(42.0), &env);
        assert!(result.is_ok());
    }

    #[test]
    fn test_class_set_static_property_without_setter() {
        use crate::ast::Class;
        // Build a class without a static setter
        let class = Class {
            name: Some("C".to_string()),
            super_class: None,
            body: vec![],
        };
        let class_val = ClassValue::from_ast(&class);
        let env = make_env();
        class_val.set_class_def_env(Rc::clone(&env));

        // set_static_property sets the field directly when no setter exists
        let result = class_val.set_static_property("foo", Value::Number(42.0), &env);
        assert!(result.is_ok());
        assert_eq!(class_val.get_static_field("foo"), Some(Value::Number(42.0)));
    }
}
