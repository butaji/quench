//! JavaScript runtime values - the core runtime type.
//!
//! A JavaScript value - the fundamental runtime type.
//! All values are immutable handles; objects are Rc<RefCell<Object>> for mutation.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use crate::arena::ObjectId;
use crate::ast::{Class, ClassMember, PropertyKey};
use crate::value::function::{NativeFunction, NativeConstructor, ValueFunction};
use crate::value::object::Object;

#[allow(unused_imports)] // Re-exported for external use
pub use crate::value::convert::{to_js_string, to_bool, to_number, strict_eq, loose_eq, to_primitive};

/// A JavaScript value - the fundamental runtime type.
/// All values are immutable handles; objects are Rc<RefCell<Object>> for mutation.
#[derive(Debug, Clone)]
pub enum Value {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    /// Objects are reference-counted with interior mutability
    Object(Rc<RefCell<Object>>),
    /// Arena-resident object handles used by the shadow-tree path
    ObjectId(ObjectId),
    /// Functions hold their closure environment and have cached prototypes
    Function(ValueFunction),
    /// Native functions (host functions) are Arc-wrapped closures
    NativeFunction(Rc<NativeFunction>),
    /// Native constructors (Date, Error, etc.) - have a prototype property
    NativeConstructor(Rc<NativeConstructor>),
    /// Symbols for unique property keys
    Symbol(String),
    /// ES6 class - callable constructor with prototype chain
    Class(ClassValue),
}

/// ES6 class representation
/// Holds the class definition and provides methods to create instances
#[derive(Debug, Clone)]
pub struct ClassValue {
    /// Class name (optional, for named class expressions)
    pub name: Option<String>,
    /// Constructor parameters
    pub constructor_params: Vec<String>,
    /// Constructor body statements
    pub constructor_body: Vec<crate::ast::Statement>,
    /// Instance methods (name -> (params, body))
    pub methods: Vec<(PropertyKey, Vec<String>, Vec<crate::ast::Statement>)>,
    /// Static methods (name -> (params, body))
    pub static_methods: Vec<(PropertyKey, Vec<String>, Vec<crate::ast::Statement>)>,
    /// Instance getters (name -> body)
    pub getters: Vec<(PropertyKey, Vec<crate::ast::Statement>)>,
    /// Instance setters (name -> (param, body))
    pub setters: Vec<(PropertyKey, String, Vec<crate::ast::Statement>)>,
    /// Superclass expression (None for no extends)
    pub(crate) super_class: Option<Box<crate::ast::Expression>>,
    /// Cached prototype object for instanceof checks
    /// Uses Rc so all clones of ClassValue share the same cache
    pub(crate) prototype_cell: std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<std::cell::RefCell<Object>>>>>,
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

        for member in &class.body {
            match member {
                ClassMember::Constructor { params, body } => {
                    constructor_params = params.clone();
                    constructor_body = body.clone();
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
            }
        }

        ClassValue {
            name: class.name.clone(),
            constructor_params,
            constructor_body,
            methods,
            static_methods,
            getters,
            setters,
            super_class: class.super_class.clone(),
            // All ClassValue clones share the same cache via Rc
            prototype_cell: std::rc::Rc::new(std::cell::RefCell::new(None)),
        }
    }
    
    /// Set the cached prototype for this class (shared across all clones)
    pub fn set_prototype(&self, proto: std::rc::Rc<std::cell::RefCell<Object>>) {
        let mut cell = self.prototype_cell.borrow_mut();
        *cell = Some(proto);
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
            (Value::ObjectId(a), Value::ObjectId(b)) => a == b,
            _ => false,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", to_js_string(self))
    }
}
