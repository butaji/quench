//! ValueFunction - JavaScript function values.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use crate::ast::{ArrowBody, Param, Statement};
use crate::env::Environment;
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
    /// Cached prototype object
    proto_cell: ProtoCellRef,
    /// Additional properties (e.g., sameValue, notSameValue on assert)
    /// Wrapped in Rc<RefCell> so clones share mutations (see Clone impl).
    properties: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, Value>>>,
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
            proto_cell: self.proto_cell.clone(),
            properties: std::rc::Rc::clone(&self.properties),
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
        if p.default.is_some() {
            break;
        }
        count += 1;
    }
    count as f64
}

impl ValueFunction {
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
        ValueFunction {
            name,
            params,
            body: std::rc::Rc::new(body),
            arrow_body: std::rc::Rc::new(None),
            closure,
            is_arrow: false,
            is_async,
            is_generator,
            strict: false,
            is_method: false,
            proto_cell: ProtoCellRef::Strong(Rc::new(RefCell::new(None))),
            properties: std::rc::Rc::new(std::cell::RefCell::new(props)),
        }
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
            proto_cell: ProtoCellRef::Strong(Rc::new(RefCell::new(None))),
            properties: std::rc::Rc::new(std::cell::RefCell::new(props)),
        }
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

    /// Build the prototype object for this function.
    fn new_prototype_object(&self) -> Object {
        let mut proto = Object::new(ObjectKind::Ordinary);
        proto.set("constructor", self.constructor_value());
        if let Some(func_proto) = crate::builtins::get_function_prototype() {
            proto.prototype = Some(func_proto);
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
        self.properties.borrow().get(key).cloned()
    }

    /// Set a property on this function (e.g., prototype).
    /// Per ES spec §16.1, class methods (is_method=true) have restricted
    /// `caller` and `arguments` properties.
    pub fn set_property(&self, key: &str, value: Value) -> Result<(), crate::value::JsError> {
        if self.is_method && (key == "caller" || key == "arguments") {
            let (_, err) = crate::value::create_js_error_with_type(
                "'caller' and 'arguments' are restricted properties and cannot be set on this function",
                "TypeError",
            );
            return Err(err);
        }
        self.with_mut(|props| {
            props.insert(key.to_string(), value);
        });
        Ok(())
    }

    /// Remove a property. Returns true if it was present.
    pub fn remove_property(&self, key: &str) -> bool {
        self.properties.borrow_mut().remove(key).is_some()
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
            crate::ast::ClassMember::Constructor { params, body } => {
                let body_str = body
                    .iter()
                    .map(stmt_to_string)
                    .collect::<Vec<_>>()
                    .join("; ");
                format!("constructor({}) {{{}}}", params.join(", "), body_str)
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
                format!("set {}({}) {{{}}}", name_str, param, body_str)
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
                format!("static set {}({}) {{{}}}", name_str, param, body_str)
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
                            crate::ast::PropertyValue::Value(e) => {
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
                                format!("set {}({}) {{ {} }}", key_str, param, body_str)
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
            Expression::ArrowFunction { params, body } => {
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
            Expression::JsxElement { .. } => "[JsxElement]".to_string(),
            Expression::JsxFragment { .. } => "[JsxFragment]".to_string(),
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
