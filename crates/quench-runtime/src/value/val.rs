//! JavaScript runtime values - the core runtime type.
//!
//! A JavaScript value - the fundamental runtime type.
//! All values are immutable handles; objects are Rc<RefCell<Object>> for mutation.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use num_bigint::BigInt;

use crate::ast::{Class, ClassMember, Param, PropertyKey};
use crate::value::function::{NativeConstructor, NativeFunction, ValueFunction};
use crate::value::object::Object;

#[allow(unused_imports)] // Re-exported for external use
pub use crate::value::convert::{
    loose_eq, strict_eq, to_bool, to_js_string, to_number, to_primitive,
};

/// ECMA-262 6.1.6 Symbol type — unique, immutable, optionally described.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Symbol {
    pub desc: Option<Rc<str>>,
    pub global: bool, // Symbol.for / Symbol.keyFor
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
    Class(ClassValue),
    /// BigInt arbitrary-precision integer
    BigInt(Rc<BigInt>),
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
    /// Instance methods (name -> (params, body))
    pub methods: Vec<(PropertyKey, Vec<Param>, Vec<crate::ast::Statement>)>,
    /// Static methods (name -> (params, body))
    pub static_methods: Vec<(PropertyKey, Vec<Param>, Vec<crate::ast::Statement>)>,
    /// Instance getters (name -> body)
    pub getters: Vec<(PropertyKey, Vec<crate::ast::Statement>)>,
    /// Instance setters (name -> (param, body))
    pub setters: Vec<(PropertyKey, String, Vec<crate::ast::Statement>)>,
    /// Instance fields (name -> value expression)
    pub instance_fields: Vec<(PropertyKey, crate::ast::Expression)>,
    /// Static fields (name -> value expression)
    pub static_fields: Vec<(PropertyKey, crate::ast::Expression)>,
    /// Superclass expression (None for no extends)
    pub(crate) super_class: Option<Box<crate::ast::Expression>>,
    /// Cached prototype object for instanceof checks
    /// Uses Rc so all clones of ClassValue share the same cache
    pub(crate) prototype_cell:
        std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<std::cell::RefCell<Object>>>>>,
    /// Static field values (name -> value), initialized during class expression evaluation
    pub(crate) static_properties_cell: std::rc::Rc<std::cell::RefCell<HashMap<String, Value>>>,
    /// Deleted property names (configurable properties like "name" that were deleted)
    pub(crate) deleted_properties:
        std::rc::Rc<std::cell::RefCell<std::collections::HashSet<String>>>,
}

impl ClassValue {
    /// Create a ClassValue from an AST Class node
    #[allow(dead_code)]
    pub fn from_ast(class: &Class) -> Self {
        let mut constructor_params = Vec::new();
        let mut constructor_body = Vec::new();
        let mut methods = Vec::new();
        let mut static_methods = Vec::new();
        let mut getters = Vec::new();
        let mut setters = Vec::new();
        let mut instance_fields = Vec::new();
        let mut static_fields = Vec::new();

        fill_members_from_ast(
            &class.body,
            &mut constructor_params,
            &mut constructor_body,
            &mut methods,
            &mut static_methods,
            &mut getters,
            &mut setters,
            &mut instance_fields,
            &mut static_fields,
        );

        ClassValue {
            id: CLASS_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            name: class.name.clone(),
            constructor_params,
            constructor_body,
            methods,
            static_methods,
            getters,
            setters,
            instance_fields,
            static_fields,
            super_class: class.super_class.clone(),
            prototype_cell: std::rc::Rc::new(std::cell::RefCell::new(None)),
            static_properties_cell: std::rc::Rc::new(std::cell::RefCell::new(HashMap::new())),
            deleted_properties: std::rc::Rc::new(std::cell::RefCell::new(
                std::collections::HashSet::new(),
            )),
        }
    }

    /// Set the cached prototype for this class (shared across all clones)
    pub fn set_prototype(&self, proto: std::rc::Rc<std::cell::RefCell<Object>>) {
        let mut cell = self.prototype_cell.borrow_mut();
        *cell = Some(proto);
    }

    /// Set a static field value on this class
    pub fn set_static_field(&self, name: &str, value: Value) {
        self.static_properties_cell
            .borrow_mut()
            .insert(name.to_string(), value);
    }

    /// Get a static field value from this class
    pub fn get_static_field(&self, name: &str) -> Option<Value> {
        self.static_properties_cell.borrow().get(name).cloned()
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
    methods: &mut Vec<(PropertyKey, Vec<Param>, Vec<crate::ast::Statement>)>,
    static_methods: &mut Vec<(PropertyKey, Vec<Param>, Vec<crate::ast::Statement>)>,
    getters: &mut Vec<(PropertyKey, Vec<crate::ast::Statement>)>,
    setters: &mut Vec<(PropertyKey, String, Vec<crate::ast::Statement>)>,
    instance_fields: &mut Vec<(PropertyKey, crate::ast::Expression)>,
    static_fields: &mut Vec<(PropertyKey, crate::ast::Expression)>,
) {
    for member in members {
        match member {
            ClassMember::Constructor { params, body } => {
                *constructor_params = params.clone();
                *constructor_body = body.clone();
            }
            ClassMember::Method { name, params, body } => {
                methods.push((name.clone(), params.clone(), body.clone()));
            }
            ClassMember::StaticMethod { name, params, body } => {
                static_methods.push((name.clone(), params.clone(), body.clone()));
            }
            ClassMember::Getter { name, body } => {
                getters.push((name.clone(), body.clone()));
            }
            ClassMember::Setter { name, param, body } => {
                setters.push((name.clone(), param.clone(), body.clone()));
            }
            ClassMember::Field { name, value } => {
                instance_fields.push((name.clone(), value.clone()));
            }
            ClassMember::StaticField { name, value } => {
                static_fields.push((name.clone(), value.clone()));
            }
        }
    }
}

impl Value {
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
        Value::Symbol(Rc::new(Symbol {
            desc: Some(Rc::from(desc)),
            global: false,
        }))
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
        let s = Symbol {
            desc: Some(Rc::from("foo")),
            global: false,
        };
        assert_eq!(s.desc.as_deref(), Some("foo"));
        assert!(!s.global);
        assert_eq!(
            Symbol {
                desc: Some(Rc::from("foo")),
                global: false
            },
            s
        );
        let sg = Symbol {
            desc: None,
            global: true,
        };
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
}
