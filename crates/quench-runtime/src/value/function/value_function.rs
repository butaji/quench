//! ValueFunction - JavaScript function values.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use crate::ast::{ArrowBody, Param, Statement};
use crate::env::Environment;
use crate::value::function::ConstructorAccessor;
use crate::value::kind::ObjectKind;
use crate::value::object::Object;
use crate::value::Value;

/// Type alias for function prototype storage
type ProtoCell = Rc<RefCell<Option<Rc<RefCell<Object>>>>>;

/// Reference to a function's cached prototype cell.
///
/// Normal clones share the cell strongly. The clone stored as the
/// prototype object's `constructor` property holds it weakly, breaking the
/// Rc cycle `function -> proto_cell -> prototype object -> constructor ->
/// proto_cell` that would otherwise leak every function prototype forever.
///
/// Known limitation: the closure cycle `function -> closure env -> function`
/// (a function whose environment binds the function itself) is still a
/// strong Rc cycle and leaks; breaking it requires a real GC.
#[derive(Clone)]
enum ProtoCellRef {
    Strong(ProtoCell),
    Weak(std::rc::Weak<RefCell<Option<Rc<RefCell<Object>>>>>),
}

impl ProtoCellRef {
    /// Get a strong reference to the cell, if it is still alive.
    fn upgrade(&self) -> Option<ProtoCell> {
        match self {
            ProtoCellRef::Strong(rc) => Some(Rc::clone(rc)),
            ProtoCellRef::Weak(w) => w.upgrade(),
        }
    }

    /// Address of the cell allocation, usable as a function identity key.
    /// A live Weak keeps the RcBox allocation reserved, so the address
    /// cannot be reused while a weak reference to it exists.
    fn as_ptr(&self) -> *const RefCell<Option<Rc<RefCell<Object>>>> {
        match self {
            ProtoCellRef::Strong(rc) => Rc::as_ptr(rc),
            ProtoCellRef::Weak(w) => w.as_ptr(),
        }
    }
}

// =============================================================================
// ValueFunction
// =============================================================================

/// Function value - holds function data with closure and cached prototype.
/// Uses interior mutability (RefCell) for the prototype to allow mutation
/// even when we only have an immutable reference to the function.
pub struct ValueFunction {
    /// Function name (for toString and debugging)
    pub name: Option<String>,
    /// Parameter list with optional defaults
    pub params: Vec<Param>,
    /// Function body (for regular functions)
    pub body: std::rc::Rc<Vec<Statement>>,
    /// Arrow function body (expression or block)
    pub arrow_body: std::rc::Rc<Option<ArrowBody>>,
    /// Closure environment - variables visible in this scope
    pub closure: Rc<RefCell<Environment>>,
    /// Whether this is an arrow function (doesn't bind its own 'this')
    pub is_arrow: bool,
    /// Whether this is an async function (wraps return value in Promise.resolve())
    pub is_async: bool,
    /// Whether this is a generator function (has yield capability)
    pub is_generator: bool,
    /// Strictness captured where the function was DEFINED (per spec),
    /// never inherited from the call site.
    pub strict: bool,
    /// Whether this function was created from a MethodDefinition (class method,
    /// getter, or setter). Such functions have restricted `caller` and
    /// `arguments` properties per ES spec §16.1.
    pub is_method: bool,
    /// GeneratorFunction-constructed functions omit `.prototype.constructor`.
    pub empty_prototype: bool,
    /// Cached prototype object
    proto_cell: ProtoCellRef,
    /// Instance [[Prototype]] when created via `class Sub extends Function` super()
    instance_proto: Option<Rc<RefCell<Object>>>,
    /// Additional properties (e.g., sameValue, notSameValue on assert)
    /// Wrapped in Rc<RefCell> so clones share mutations (see Clone impl).
    properties: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, Value>>>,
    accessors:
        std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, ConstructorAccessor>>>,
}

impl Clone for ValueFunction {
    fn clone(&self) -> Self {
        // Share the same Rc<RefCell<HashMap>> with the original so deletes /
        // mutations are visible to subsequent accesses.
        ValueFunction {
            name: self.name.clone(),
            params: self.params.clone(),
            body: std::rc::Rc::clone(&self.body),
            arrow_body: std::rc::Rc::clone(&self.arrow_body),
            closure: std::rc::Rc::clone(&self.closure),
            is_arrow: self.is_arrow,
            is_async: self.is_async,
            is_generator: self.is_generator,
            strict: self.strict,
            is_method: self.is_method,
            empty_prototype: self.empty_prototype,
            proto_cell: self.proto_cell.clone(),
            instance_proto: self.instance_proto.as_ref().map(Rc::clone),
            properties: std::rc::Rc::clone(&self.properties),
            accessors: std::rc::Rc::clone(&self.accessors),
        }
    }
}

impl fmt::Debug for ValueFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ValueFunction({:?})", self.name)
    }
}

/// Per ES §14.1 ExpectedArgumentCount: count parameters until (and
/// including) the first one with a default value, then stop.
pub(crate) fn expected_argument_count(params: &[Param]) -> f64 {
    let mut count = 0;
    for p in params {
        if p.default.is_some() || p.rest {
            break;
        }
        count += 1;
    }
    count as f64
}

impl ValueFunction {
    fn deleted_marker(key: &str) -> String {
        format!("\0deleted:{key}")
    }

    pub fn is_property_deleted(&self, key: &str) -> bool {
        self.properties
            .borrow()
            .contains_key(&Self::deleted_marker(key))
    }

    /// Create a new regular function
    pub fn new(
        name: Option<String>,
        params: Vec<Param>,
        body: Vec<Statement>,
        closure: Rc<RefCell<Environment>>,
        is_async: bool,
        is_generator: bool,
    ) -> Self {
        let length = expected_argument_count(&params);
        let mut props = std::collections::HashMap::new();
        props.insert("length".to_string(), Value::Number(length));
        if let Some(ref n) = name {
            props.insert("name".to_string(), Value::String(n.clone()));
        }
        // Per ES spec, function strictness comes from:
        // 1. "use strict" directive in the function body, OR
        // 2. The enclosing context is strict (module code, strict mode code, etc.)
        let has_use_strict = crate::interpreter::helpers::check_use_strict_directive(&body);
        let in_strict_context = crate::interpreter::is_strict_mode();
        let strict = has_use_strict || in_strict_context;
        ValueFunction {
            name,
            params,
            body: std::rc::Rc::new(body),
            arrow_body: std::rc::Rc::new(None),
            closure,
            is_arrow: false,
            is_async,
            is_generator,
            strict,
            is_method: false,
            empty_prototype: false,
            proto_cell: ProtoCellRef::Strong(Rc::new(RefCell::new(None))),
            instance_proto: None,
            properties: std::rc::Rc::new(std::cell::RefCell::new(props)),
            accessors: std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::new())),
        }
    }

    pub fn define_accessor(&self, key: &str, getter: Option<Value>, setter: Option<Value>) {
        self.accessors
            .borrow_mut()
            .insert(key.to_string(), ConstructorAccessor { getter, setter });
    }

    pub fn get_accessor(&self, key: &str) -> Option<ConstructorAccessor> {
        self.accessors.borrow().get(key).cloned()
    }

    pub fn set_empty_prototype(&mut self, empty: bool) {
        self.empty_prototype = empty;
    }

    pub fn set_generator_prototype(&mut self, prototype: Rc<RefCell<Object>>) {
        let instance = Rc::new(RefCell::new(Object::with_prototype(
            ObjectKind::Ordinary,
            prototype,
        )));
        self.properties
            .borrow_mut()
            .insert("prototype".to_string(), Value::Object(instance));
    }

    /// Create a new arrow function
    #[allow(clippy::boxed_local)]
    pub fn new_arrow(
        params: Vec<Param>,
        body: Box<ArrowBody>,
        closure: Rc<RefCell<Environment>>,
    ) -> Self {
        let length = expected_argument_count(&params);
        let mut props = std::collections::HashMap::new();
        props.insert("length".to_string(), Value::Number(length));
        ValueFunction {
            name: None,
            params,
            body: std::rc::Rc::new(Vec::new()),
            arrow_body: std::rc::Rc::new(Some(*body)),
            closure,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            strict: false,
            is_method: false,
            empty_prototype: false,
            proto_cell: ProtoCellRef::Strong(Rc::new(RefCell::new(None))),
            instance_proto: None,
            properties: std::rc::Rc::new(std::cell::RefCell::new(props)),
            accessors: std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::new())),
        }
    }

    /// Get the function's `.prototype` object only if it is an object (not null/undefined).
    /// Returns None when `.prototype` is not an object (to trigger intrinsic fallback,
    /// e.g. %GeneratorPrototype% for generator instances when user sets g.prototype = null).
    pub fn get_prototype_if_object(&self) -> Option<Rc<RefCell<Object>>> {
        // Check if prototype has been explicitly set to something
        if let Some(val) = self.properties.borrow().get("prototype") {
            match val {
                Value::Object(o) => return Some(Rc::clone(o)),
                _ => return None, // explicitly set to non-object (null, number, etc.)
            }
        }
        Some(self.get_prototype())
    }

    /// Get the prototype object for this function, creating it if needed.
    pub fn get_prototype(&self) -> Rc<RefCell<Object>> {
        if let Some(Value::Object(proto)) = self.properties.borrow().get("prototype") {
            return Rc::clone(proto);
        }
        if let Some(cell) = self.proto_cell.upgrade() {
            let mut cell_ref = cell.borrow_mut();
            if let Some(ref proto) = *cell_ref {
                return Rc::clone(proto);
            }
            let proto_rc = Rc::new(RefCell::new(self.new_prototype_object()));
            *cell_ref = Some(Rc::clone(&proto_rc));
            return proto_rc;
        }
        Rc::new(RefCell::new(self.new_prototype_object()))
    }

    /// Instance [[Prototype]] for builtin-subclassed function objects.
    pub fn instance_proto(&self) -> Option<Rc<RefCell<Object>>> {
        self.instance_proto.as_ref().map(Rc::clone)
    }

    pub fn set_instance_proto(&mut self, proto: Rc<RefCell<Object>>) {
        self.instance_proto = Some(proto);
    }

    /// Build the prototype object for this function.
    /// Per ES spec §19.2.4.3, a function's `.prototype` is a plain object
    /// with `constructor` pointing back to the function (unless it's a
    /// generator function, whose prototype has no own properties). Its
    /// [[Prototype]] is `Object.prototype`, NOT `Function.prototype`.
    fn new_prototype_object(&self) -> Object {
        let mut proto = Object::new(ObjectKind::Ordinary);
        // Generator functions' .prototype has no own properties per ES spec.
        if !self.empty_prototype && !self.is_generator {
            proto.set_builtin_method("constructor", self.constructor_value());
        }
        if let Some(obj_proto) = crate::builtins::get_object_prototype() {
            proto.prototype = Some(obj_proto);
        }
        if self.is_generator {
            let generator_proto = if self.is_async {
                crate::builtins::function::get_async_generator_prototype()
            } else {
                crate::builtins::function::get_generator_prototype()
            };
            if let Some(generator_proto) = generator_proto {
                proto.prototype = Some(generator_proto);
            }
        }
        proto
    }

    /// `constructor` property value for the prototype object.
    /// Holds the proto cell weakly so the prototype does not keep the
    /// function (and its own proto cell) alive forever.
    fn constructor_value(&self) -> Value {
        let mut ctor = self.clone();
        if let Some(cell) = self.proto_cell.upgrade() {
            ctor.proto_cell = ProtoCellRef::Weak(Rc::downgrade(&cell));
        }
        Value::Function(ctor)
    }

    /// Check if function has a prototype (cached)
    pub fn has_prototype(&self) -> bool {
        self.proto_cell
            .upgrade()
            .is_some_and(|cell| cell.borrow().is_some())
    }

    /// Identity key for strict equality.
    pub(crate) fn identity_ptr(&self) -> *const RefCell<Option<Rc<RefCell<Object>>>> {
        self.proto_cell.as_ptr()
    }

    /// Compute the function's length per ECMA-262 14.1 / 9.2.4
    pub fn length(&self) -> usize {
        expected_argument_count(&self.params) as usize
    }

    /// Get a property from this function (e.g., sameValue, notSameValue)
    pub fn get_property(&self, key: &str) -> Option<Value> {
        if key == "prototype" {
            if let Some(value) = self.properties.borrow().get(key) {
                return Some(value.clone());
            }
        }
        if key == "prototype"
            && !self.is_property_deleted("prototype")
            && !self.is_arrow
            && (!self.is_method || self.is_generator)
            && (!self.is_async || self.is_generator)
        {
            return Some(Value::Object(self.get_prototype()));
        }
        self.properties.borrow().get(key).cloned()
    }

    /// Own property names including non-enumerable `length`, `name`, and `prototype`.
    pub fn own_property_names(&self) -> Vec<String> {
        let mut names = if self.is_property_deleted("length") {
            Vec::new()
        } else {
            vec!["length".to_string()]
        };
        if !self.is_property_deleted("name")
            && (self.get_property("name").is_some() || self.name.is_some())
        {
            names.push("name".to_string());
        }
        // Non-arrow functions always have a .prototype property.
        if !self.is_property_deleted("prototype")
            && !self.is_arrow
            && (!self.is_method || self.is_generator)
            && (!self.is_async || self.is_generator)
        {
            names.push("prototype".to_string());
        }
        for key in self.properties.borrow().keys() {
            if !key.starts_with('\0') && key != "length" && key != "name" && key != "prototype" {
                names.push(key.clone());
            }
        }
        names
    }

    /// Set a property on this function (e.g., prototype).
    /// Per ES spec §16.1, class methods (is_method=true) have restricted
    /// `caller` and `arguments` properties.
    pub fn set_property(&self, key: &str, value: Value) -> Result<(), crate::value::JsError> {
        if self
            .properties
            .borrow()
            .contains_key(&format!("\0nonwritable:{key}"))
        {
            return Ok(());
        }
        if self.is_method && (key == "caller" || key == "arguments") {
            let (_, err) = crate::value::create_js_error_with_type(
                "'caller' and 'arguments' are restricted properties and cannot be set on this function",
                "TypeError",
            );
            return Err(err);
        }
        if key == "prototype" {
            let proto = match &value {
                Value::Object(proto) => Some(Rc::clone(proto)),
                Value::Function(proto) => Some(proto.get_prototype()),
                _ => None,
            };
            if let Some(cell) = self.proto_cell.upgrade() {
                *cell.borrow_mut() = proto;
            }
        }
        self.properties
            .borrow_mut()
            .remove(&Self::deleted_marker(key));
        let property_value = value.clone();
        self.with_mut(|props| {
            props.insert(key.to_string(), value);
        });
        if key != "name" && key != "length" && key != "prototype" {
            self.get_prototype().borrow_mut().set(key, property_value);
        }
        Ok(())
    }

    pub fn mark_nonwritable(&self, key: &str) {
        self.properties
            .borrow_mut()
            .insert(format!("\0nonwritable:{key}"), Value::Undefined);
    }

    /// Remove a property. Returns true if it was present.
    pub fn remove_property(&self, key: &str) -> bool {
        if key == "prototype"
            && !self.is_arrow
            && (!self.is_method || self.is_generator)
            && (!self.is_async || self.is_generator)
        {
            return false;
        }
        let removed = self.properties.borrow_mut().remove(key).is_some();
        if matches!(key, "name" | "length" | "prototype") {
            let marker = Self::deleted_marker(key);
            let was_deleted = self.properties.borrow_mut().remove(&marker).is_some();
            if was_deleted {
                return false;
            }
            self.properties
                .borrow_mut()
                .insert(marker, Value::Undefined);
            return true;
        }
        removed
    }

    /// Access properties with mutable borrow.
    fn with_mut<F>(&self, f: F)
    where
        F: FnOnce(&mut std::collections::HashMap<String, Value>),
    {
        let mut map = self.properties.borrow_mut();
        f(&mut map);
    }

    /// Get the function's source text for Function.prototype.toString.
    /// Generates a representation from AST components.
    pub fn source_text(&self) -> String {
        generate_source_text(self)
    }
}

/// Generate a string representation of this function from its AST components.
fn generate_source_text(f: &ValueFunction) -> String {
    use crate::ast::{ArrowBody, Expression, Statement};

    fn fmt_param(name: &str, default: &Option<Box<Expression>>, rest: bool) -> String {
        if rest {
            format!("...{}", name)
        } else if let Some(def) = default {
            format!("{} = {}", name, expr_to_string(def))
        } else {
            name.to_string()
        }
    }

    fn fmt_params(params: &[crate::ast::Param]) -> String {
        params
            .iter()
            .map(|p| fmt_param(&p.name, &p.default, p.rest))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn class_member_to_string(member: &crate::ast::ClassMember) -> String {
        match member {
            crate::ast::ClassMember::Constructor { params, body, .. } => {
                let body_str = body
                    .iter()
                    .map(stmt_to_string)
                    .collect::<Vec<_>>()
                    .join("; ");
                format!("constructor({}) {{{}}}", fmt_params(params), body_str)
            }
            crate::ast::ClassMember::Method {
                name,
                params,
                body,
                is_async,
                is_generator,
            } => {
                let prefix = match (*is_async, *is_generator) {
                    (true, true) => "async function*",
                    (true, false) => "async ",
                    (false, true) => "function* ",
                    (false, false) => "",
                };
                let name_str = prop_key_to_string(name);
                let body_str = body
                    .iter()
                    .map(stmt_to_string)
                    .collect::<Vec<_>>()
                    .join("; ");
                format!(
                    "{}{}({}) {{{}}}",
                    prefix,
                    name_str,
                    fmt_params(params),
                    body_str
                )
            }
            crate::ast::ClassMember::Getter { name, body } => {
                let name_str = prop_key_to_string(name);
                let body_str = body
                    .iter()
                    .map(stmt_to_string)
                    .collect::<Vec<_>>()
                    .join("; ");
                format!("get {}() {{{}}}", name_str, body_str)
            }
            crate::ast::ClassMember::Setter { name, param, body } => {
                let name_str = prop_key_to_string(name);
                let body_str = body
                    .iter()
                    .map(stmt_to_string)
                    .collect::<Vec<_>>()
                    .join("; ");
                format!("set {}({}) {{{}}}", name_str, param.name, body_str)
            }
            crate::ast::ClassMember::StaticMethod {
                name,
                params,
                body,
                is_async,
                is_generator,
            } => {
                let prefix = match (*is_async, *is_generator) {
                    (true, true) => "async static function*",
                    (true, false) => "async static ",
                    (false, true) => "static function* ",
                    (false, false) => "static ",
                };
                let name_str = prop_key_to_string(name);
                let body_str = body
                    .iter()
                    .map(stmt_to_string)
                    .collect::<Vec<_>>()
                    .join("; ");
                format!(
                    "{}{}({}) {{{}}}",
                    prefix,
                    name_str,
                    fmt_params(params),
                    body_str
                )
            }
            crate::ast::ClassMember::Field { name, value } => {
                let name_str = prop_key_to_string(name);
                format!("{} = {}", name_str, expr_to_string(value))
            }
            crate::ast::ClassMember::StaticField { name, value } => {
                let name_str = prop_key_to_string(name);
                format!("static {} = {}", name_str, expr_to_string(value))
            }
            crate::ast::ClassMember::StaticGetter { name, body } => {
                let name_str = prop_key_to_string(name);
                let body_str = body
                    .iter()
                    .map(stmt_to_string)
                    .collect::<Vec<_>>()
                    .join("; ");
                format!("static get {}() {{{}}}", name_str, body_str)
            }
            crate::ast::ClassMember::StaticSetter { name, param, body } => {
                let name_str = prop_key_to_string(name);
                let body_str = body
                    .iter()
                    .map(stmt_to_string)
                    .collect::<Vec<_>>()
                    .join("; ");
                format!("static set {}({}) {{{}}}", name_str, param.name, body_str)
            }
            crate::ast::ClassMember::StaticBlock { body } => {
                let body_str = body
                    .iter()
                    .map(stmt_to_string)
                    .collect::<Vec<_>>()
                    .join("; ");
                format!("static {{ {} }}", body_str)
            }
        }
    }

    fn stmt_to_string(stmt: &Statement) -> String {
        match stmt {
            Statement::Return(opt_expr) => {
                if let Some(expr) = opt_expr {
                    format!("return {}", expr_to_string(expr))
                } else {
                    "return".to_string()
                }
            }
            Statement::Expression(expr) => expr_to_string(expr),
            Statement::Block(stmts) => {
                let inner = stmts
                    .iter()
                    .map(stmt_to_string)
                    .collect::<Vec<_>>()
                    .join("; ");
                format!("{{ {} }}", inner)
            }
            Statement::If {
                condition,
                consequent,
                alternate,
            } => {
                let s = format!(
                    "if ({}) {}",
                    expr_to_string(condition),
                    stmt_to_string(consequent)
                );
                if let Some(alt) = alternate {
                    format!("{} else {}", s, stmt_to_string(alt))
                } else {
                    s
                }
            }
            Statement::While { condition, body } => {
                format!(
                    "while ({}) {}",
                    expr_to_string(condition),
                    stmt_to_string(body)
                )
            }
            Statement::For {
                init,
                condition,
                update,
                body,
            } => {
                let init_str = match init {
                    Some(crate::ast::ForInit::Expression(e)) => expr_to_string(e),
                    Some(crate::ast::ForInit::VarDeclaration { kind, name, init }) => {
                        let k = match kind {
                            crate::ast::VarKind::Var => "var",
                            crate::ast::VarKind::Let => "let",
                            crate::ast::VarKind::Const => "const",
                        };
                        match init {
                            Some(i) => format!("{} {} = {}", k, name, expr_to_string(i)),
                            None => format!("{} {}", k, name),
                        }
                    }
                    Some(crate::ast::ForInit::PatternDeclaration { .. }) => {
                        "[PatternDeclaration]".to_string()
                    }
                    Some(crate::ast::ForInit::DeclarationList { .. }) => {
                        "[DeclarationList]".to_string()
                    }
                    None => String::new(),
                };
                let cond_str = condition
                    .as_ref()
                    .map(|c| expr_to_string(c))
                    .unwrap_or_default();
                let upd_str = update
                    .as_ref()
                    .map(|u| expr_to_string(u))
                    .unwrap_or_default();
                format!(
                    "for ({}; {}; {}) {}",
                    init_str,
                    cond_str,
                    upd_str,
                    stmt_to_string(body)
                )
            }
            Statement::ForIn {
                variable,
                object,
                body,
            } => {
                format!(
                    "for ({} in {}) {}",
                    expr_to_string(variable),
                    expr_to_string(object),
                    stmt_to_string(body)
                )
            }
            Statement::VarDeclaration { kind, name, init } => {
                let k = match kind {
                    crate::ast::VarKind::Var => "var",
                    crate::ast::VarKind::Let => "let",
                    crate::ast::VarKind::Const => "const",
                };
                match init {
                    Some(i) => format!("{} {} = {}", k, name, expr_to_string(i)),
                    None => format!("{} {}", k, name),
                }
            }
            Statement::PatternDeclaration { kind, init, .. } => {
                let k = match kind {
                    crate::ast::VarKind::Var => "var",
                    crate::ast::VarKind::Let => "let",
                    crate::ast::VarKind::Const => "const",
                };
                match init {
                    Some(i) => format!("{} [...] = {}", k, expr_to_string(i)),
                    None => format!("{} [...]", k),
                }
            }
            Statement::FunctionDeclaration {
                name,
                params,
                body,
                is_async,
                is_generator,
            } => {
                let prefix = match (*is_async, *is_generator) {
                    (true, true) => "async function*",
                    (true, false) => "async function",
                    (false, true) => "function*",
                    (false, false) => "function",
                };
                let body_str = body
                    .iter()
                    .map(stmt_to_string)
                    .collect::<Vec<_>>()
                    .join("; ");
                format!(
                    "{} {}({}) {{ {} }}",
                    prefix,
                    name,
                    fmt_params(params),
                    body_str
                )
            }
            Statement::Try {
                body,
                param,
                handler,
                finalizer,
            } => {
                let catch_str = handler
                    .as_ref()
                    .map(|h| match param {
                        Some(p) => format!(" catch ({}) {}", p, stmt_to_string(h)),
                        None => format!(" catch {}", stmt_to_string(h)),
                    })
                    .unwrap_or_default();
                let finally_str = finalizer
                    .as_ref()
                    .map(|f| format!(" finally {}", stmt_to_string(f)))
                    .unwrap_or_default();
                format!(
                    "try {{ {} }}{}{}",
                    stmt_to_string(body),
                    catch_str,
                    finally_str
                )
            }
            Statement::Throw(expr) => {
                format!("throw {}", expr_to_string(expr))
            }
            Statement::Break(_) => "break".to_string(),
            Statement::Continue(_) => "continue".to_string(),
            Statement::Labeled { label, body } => {
                format!("{}: {}", label, stmt_to_string(body))
            }
            Statement::DoWhile {
                body, condition, ..
            } => {
                format!(
                    "do {} while ({})",
                    stmt_to_string(body),
                    expr_to_string(condition)
                )
            }
            Statement::With { object, body } => {
                format!("with ({}) {}", expr_to_string(object), stmt_to_string(body))
            }
            Statement::Empty => String::new(),
            Statement::Dispose { .. } => String::new(),
            Statement::RegisterDispose { .. } => String::new(),
            Statement::SequenceDecls(_) => String::new(),
            Statement::Export(_) => String::new(),
            Statement::Import { .. } => String::new(),
            Statement::ClassDeclaration { name, class } => {
                let extends_str = class
                    .super_class
                    .as_ref()
                    .map(|e| format!(" extends {}", expr_to_string(e)))
                    .unwrap_or_default();
                let member_strs: Vec<String> =
                    class.body.iter().map(class_member_to_string).collect();
                format!("class {}{} {{{}}}", name, extends_str, member_strs.join(""))
            }
        }
    }

    fn prop_key_to_string(key: &crate::ast::PropertyKey) -> String {
        match key {
            crate::ast::PropertyKey::Ident(s) => s.clone(),
            crate::ast::PropertyKey::String(s) => format!("\"{}\"", s),
            crate::ast::PropertyKey::Number(n) => n.to_string(),
            crate::ast::PropertyKey::Computed(e) => expr_to_string(e),
        }
    }

    fn expr_to_string(expr: &Expression) -> String {
        match expr {
            Expression::Number(n) => n.to_string(),
            Expression::String(s) => format!(
                "\"{}\"",
                s.replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', "\\n")
            ),
            Expression::Boolean(b) => b.to_string(),
            Expression::Null => "null".to_string(),
            Expression::Undefined => "undefined".to_string(),
            Expression::Identifier(id) => id.clone(),
            Expression::Parenthesized(expr) => format!("({})", expr_to_string(expr)),
            Expression::BigInt(s) => format!("{}n", s),
            Expression::RegExp { pattern, flags } => format!("/{}/{}", pattern, flags),
            Expression::Elision => String::new(),
            Expression::Binary { op, left, right } => {
                let op_str = match op {
                    crate::ast::BinaryOp::And => "&&",
                    crate::ast::BinaryOp::Or => "||",
                    crate::ast::BinaryOp::Eq => "==",
                    crate::ast::BinaryOp::Neq => "!=",
                    crate::ast::BinaryOp::LooseEq => "==",
                    crate::ast::BinaryOp::StrictEq => "===",
                    crate::ast::BinaryOp::StrictNeq => "!==",
                    crate::ast::BinaryOp::Lt => "<",
                    crate::ast::BinaryOp::Gt => ">",
                    crate::ast::BinaryOp::Le => "<=",
                    crate::ast::BinaryOp::Ge => ">=",
                    crate::ast::BinaryOp::Add => "+",
                    crate::ast::BinaryOp::Sub => "-",
                    crate::ast::BinaryOp::Mul => "*",
                    crate::ast::BinaryOp::Div => "/",
                    crate::ast::BinaryOp::Mod => "%",
                    crate::ast::BinaryOp::BitAnd => "&",
                    crate::ast::BinaryOp::BitOr => "|",
                    crate::ast::BinaryOp::BitXor => "^",
                    crate::ast::BinaryOp::Shl => "<<",
                    crate::ast::BinaryOp::Shr => ">>",
                    crate::ast::BinaryOp::Ushr => ">>>",
                    crate::ast::BinaryOp::In => "in",
                    crate::ast::BinaryOp::Instanceof => "instanceof",
                    crate::ast::BinaryOp::NullishCoalescing => "??",
                    crate::ast::BinaryOp::Pow => "**",
                };
                format!(
                    "({} {} {})",
                    expr_to_string(left),
                    op_str,
                    expr_to_string(right)
                )
            }
            Expression::PrivateIn { name, right } => {
                format!("{} in {}", name, expr_to_string(right))
            }
            Expression::Unary { op, argument } => {
                let op_str = match op {
                    crate::ast::UnaryOp::Not => "!",
                    crate::ast::UnaryOp::Neg => "-",
                    crate::ast::UnaryOp::Plus => "+",
                    crate::ast::UnaryOp::BitNot => "~",
                    crate::ast::UnaryOp::Typeof => "typeof",
                    crate::ast::UnaryOp::Void => "void",
                    crate::ast::UnaryOp::Delete => "delete",
                };
                format!("({} {})", op_str, expr_to_string(argument))
            }
            Expression::Assignment { left, right } => {
                format!("{} = {}", expr_to_string(left), expr_to_string(right))
            }
            Expression::CompoundAssignment { op, left, right } => {
                let op_str = match op {
                    crate::ast::CompoundOp::Add => "+=",
                    crate::ast::CompoundOp::Sub => "-=",
                    crate::ast::CompoundOp::Mul => "*=",
                    crate::ast::CompoundOp::Pow => "**=",
                    crate::ast::CompoundOp::Div => "/=",
                    crate::ast::CompoundOp::Mod => "%=",
                    crate::ast::CompoundOp::BitAnd => "&=",
                    crate::ast::CompoundOp::BitOr => "|=",
                    crate::ast::CompoundOp::BitXor => "^=",
                    crate::ast::CompoundOp::Shl => "<<=",
                    crate::ast::CompoundOp::Shr => ">>=",
                    crate::ast::CompoundOp::Ushr => ">>>=",
                    crate::ast::CompoundOp::LogicalOrAssign => "||=",
                    crate::ast::CompoundOp::LogicalAndAssign => "&&=",
                    crate::ast::CompoundOp::NullishCoalescingAssign => "??=",
                };
                format!(
                    "({} {} {})",
                    expr_to_string(left),
                    op_str,
                    expr_to_string(right)
                )
            }
            Expression::LogicalCompoundAssignment { op, left, right } => {
                let op_str = match op {
                    crate::ast::CompoundOp::LogicalOrAssign => "||=",
                    crate::ast::CompoundOp::LogicalAndAssign => "&&=",
                    crate::ast::CompoundOp::NullishCoalescingAssign => "??=",
                    _ => unreachable!(),
                };
                format!(
                    "({} {} {})",
                    expr_to_string(left),
                    op_str,
                    expr_to_string(right)
                )
            }
            Expression::Call { callee, arguments } => {
                let args = arguments
                    .iter()
                    .map(expr_to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}({})", expr_to_string(callee), args)
            }
            Expression::New {
                constructor,
                arguments,
            } => {
                let args = arguments
                    .iter()
                    .map(expr_to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("new {}({})", expr_to_string(constructor), args)
            }
            Expression::Member {
                object,
                property,
                computed,
            } => {
                if *computed {
                    format!(
                        "{}[{}]",
                        expr_to_string(object),
                        prop_key_to_string(property)
                    )
                } else {
                    match property {
                        crate::ast::PropertyKey::Ident(s) => {
                            format!("{}.{}", expr_to_string(object), s)
                        }
                        crate::ast::PropertyKey::String(s) => {
                            format!("{}.{}", expr_to_string(object), s)
                        }
                        crate::ast::PropertyKey::Number(n) => {
                            format!("{}.{}", expr_to_string(object), n)
                        }
                        crate::ast::PropertyKey::Computed(e) => {
                            format!("{}[{}]", expr_to_string(object), expr_to_string(e))
                        }
                    }
                }
            }
            Expression::Conditional {
                condition,
                consequent,
                alternate,
            } => {
                format!(
                    "({} ? {} : {})",
                    expr_to_string(condition),
                    expr_to_string(consequent),
                    expr_to_string(alternate)
                )
            }
            Expression::Update {
                op,
                argument,
                prefix,
            } => {
                let op_str = match op {
                    crate::ast::UpdateOp::Increment => "++",
                    crate::ast::UpdateOp::Decrement => "--",
                };
                if *prefix {
                    format!("{}{}", op_str, expr_to_string(argument))
                } else {
                    format!("{}{}", expr_to_string(argument), op_str)
                }
            }
            Expression::Array(arr) => {
                let els: Vec<String> = arr.iter().map(expr_to_string).collect();
                format!("[{}]", els.join(","))
            }
            Expression::Object(props) => {
                let prop_strs: Vec<String> = props
                    .iter()
                    .map(|(k, v)| {
                        let key_str = match k {
                            crate::ast::PropertyKey::Ident(s) => s.clone(),
                            crate::ast::PropertyKey::String(s) => format!("\"{}\"", s),
                            crate::ast::PropertyKey::Number(n) => n.to_string(),
                            crate::ast::PropertyKey::Computed(e) => {
                                format!("[{}]", expr_to_string(e))
                            }
                        };
                        match v {
                            crate::ast::PropertyValue::Value(e)
                            | crate::ast::PropertyValue::Shorthand(e)
                            | crate::ast::PropertyValue::Method(e) => {
                                format!("{}: {}", key_str, expr_to_string(e))
                            }
                            crate::ast::PropertyValue::Getter { params: _, body } => {
                                let body_str = body
                                    .iter()
                                    .map(stmt_to_string)
                                    .collect::<Vec<_>>()
                                    .join("; ");
                                format!("get {}() {{ {} }}", key_str, body_str)
                            }
                            crate::ast::PropertyValue::Setter { param, body } => {
                                let body_str = body
                                    .iter()
                                    .map(stmt_to_string)
                                    .collect::<Vec<_>>()
                                    .join("; ");
                                format!("set {}({}) {{ {} }}", key_str, param.name, body_str)
                            }
                            crate::ast::PropertyValue::Spread(e) => {
                                format!("...{}", expr_to_string(e))
                            }
                        }
                    })
                    .collect();
                format!("{{{}}}", prop_strs.join(", "))
            }
            Expression::FunctionExpression {
                name,
                params,
                body,
                is_async,
                is_generator,
            } => {
                let prefix = match (*is_async, *is_generator) {
                    (true, true) => "async function*",
                    (true, false) => "async function",
                    (false, true) => "function*",
                    (false, false) => "function",
                };
                let name_str = name.as_ref().map(|n| format!(" {}", n)).unwrap_or_default();
                let body_str = body
                    .iter()
                    .map(stmt_to_string)
                    .collect::<Vec<_>>()
                    .join("; ");
                format!(
                    "{} {}({}) {{ {} }}",
                    prefix,
                    name_str,
                    fmt_params(params),
                    body_str
                )
            }
            Expression::ArrowFunction { params, body, .. } => {
                let body_str = match body.as_ref() {
                    ArrowBody::Expression(e) => expr_to_string(e),
                    ArrowBody::Block(stmts) => {
                        let inner = stmts
                            .iter()
                            .map(stmt_to_string)
                            .collect::<Vec<_>>()
                            .join("; ");
                        format!("{{ {} }}", inner)
                    }
                };
                format!("({}) => {}", fmt_params(params), body_str)
            }
            Expression::Sequence(exprs) => exprs
                .iter()
                .map(expr_to_string)
                .collect::<Vec<_>>()
                .join(", "),
            Expression::Class(_) => "[Class]".to_string(),
            Expression::BlockExpr(stmts) => {
                let inner = stmts
                    .iter()
                    .map(stmt_to_string)
                    .collect::<Vec<_>>()
                    .join("; ");
                format!("{{ {} }}", inner)
            }
            Expression::ArrayPattern(_) => "[ArrayPattern]".to_string(),
            Expression::ObjectPattern(_) => "[ObjectPattern]".to_string(),
            Expression::ForOf {
                variable,
                iterable,
                body,
                ..
            } => {
                format!(
                    "for ({} of {}) {}",
                    expr_to_string(variable),
                    expr_to_string(iterable),
                    stmt_to_string(body)
                )
            }
            Expression::ForIn {
                variable,
                object,
                body,
                ..
            } => {
                format!(
                    "for ({} in {}) {}",
                    expr_to_string(variable),
                    expr_to_string(object),
                    stmt_to_string(body)
                )
            }
            Expression::Yield(opt_expr) => {
                if let Some(e) = opt_expr {
                    format!("yield {}", expr_to_string(e))
                } else {
                    "yield".to_string()
                }
            }
            Expression::YieldDelegate(expr) => {
                format!("yield* {}", expr_to_string(expr))
            }
            Expression::Spread(expr) => {
                format!("...{}", expr_to_string(expr))
            }
            Expression::Await(expr) => {
                format!("await {}", expr_to_string(expr))
            }
        }
    }

    if f.is_arrow {
        let body_str = match f.arrow_body.as_ref() {
            Some(ArrowBody::Expression(e)) => expr_to_string(e),
            Some(ArrowBody::Block(stmts)) => {
                let inner = stmts
                    .iter()
                    .map(stmt_to_string)
                    .collect::<Vec<_>>()
                    .join("; ");
                format!("{{ {} }}", inner)
            }
            None => "{}".to_string(),
        };
        format!("({}) => {}", fmt_params(&f.params), body_str)
    } else {
        let (keyword, name_str) = match (f.is_async, f.is_generator) {
            (true, true) => ("async function*", f.name.as_deref().unwrap_or("")),
            (true, false) => ("async function", f.name.as_deref().unwrap_or("")),
            (false, true) => ("function*", f.name.as_deref().unwrap_or("")),
            (false, false) => ("function", f.name.as_deref().unwrap_or("")),
        };
        let body_str = f
            .body
            .iter()
            .map(stmt_to_string)
            .collect::<Vec<_>>()
            .join("; ");
        if body_str.is_empty() {
            format!("{} {}({}) {{}}", keyword, name_str, fmt_params(&f.params))
        } else {
            format!(
                "{} {}({}) {{{}}}",
                keyword,
                name_str,
                fmt_params(&f.params),
                body_str
            )
        }
    }
}
