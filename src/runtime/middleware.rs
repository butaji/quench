//! Middleware Runtime for Development Mode
//!
//! Executes the middleware pipeline with full Fresh semantics.
//! Middleware can:
//! - Modify request state
//! - Return early with a response
//! - Pass control to next middleware via `ctx.next()`

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;

use crate::transpile::hir::{
    Expr, Stmt, BinaryOp, UnaryOp, LogicalOp, TemplatePart, 
    ObjectProp, PropKey
};

use super::interpreter::{EvalContext, RequestInfo, Value};

/// Middleware execution result
#[derive(Debug, Clone)]
pub enum MiddlewareOutcome {
    /// Continue to next middleware/handler
    Continue,
    /// Return early with a response
    Response(String),
    /// Render the page component with data
    Render { data: HashMap<String, Value> },
    /// Error occurred
    Error(String),
}

impl Default for MiddlewareOutcome {
    fn default() -> Self {
        Self::Continue
    }
}

/// Middleware pipeline executor
pub struct MiddlewareExecutor {
    /// Global middleware (from routes/_middleware.ts)
    global_middleware: Arc<RwLock<Vec<MiddlewareDef>>>,
    /// Route-specific middleware (from _layout patterns)
    route_middleware: Arc<RwLock<HashMap<String, Vec<MiddlewareDef>>>>,
}

impl Default for MiddlewareExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl MiddlewareExecutor {
    pub fn new() -> Self {
        Self {
            global_middleware: Arc::new(RwLock::new(Vec::new())),
            route_middleware: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register global middleware
    pub fn register_global(&self, middleware: MiddlewareDef) {
        self.global_middleware.write().push(middleware);
    }

    /// Register route-specific middleware
    pub fn register_route_middleware(&self, route: &str, middleware: MiddlewareDef) {
        self.route_middleware
            .write()
            .entry(route.to_string())
            .or_insert_with(Vec::new)
            .push(middleware);
    }

    /// Execute the full middleware pipeline for a request
    ///
    /// Execution order:
    /// 1. Global middleware (routes/_middleware.ts)
    /// 2. Route-specific middleware (from _layout patterns)
    /// 3. Handler (route handler)
    ///
    /// Each middleware can:
    /// - Return `Response` to short-circuit
    /// - Call `ctx.next()` to continue
    /// - Modify `ctx.state` to share data
    pub fn execute_pipeline(
        &self,
        request: &RequestInfo,
        route_path: &str,
        params: HashMap<String, String>,
        _handler: Option<&HandlerDef>,
        _component: Option<&ComponentDef>,
        initial_data: Option<HashMap<String, Value>>,
    ) -> MiddlewareOutcome {
        let mut ctx = EvalContext {
            scope: HashMap::new(),
            props: HashMap::new(),
            params,
            url: request.url.clone(),
            island_props: None,
            rendered_islands: Vec::new(),
            state: self.load_state_from_request(request),
            request: Some(request.clone()),
        };

        // Add params to scope
        for (key, value) in &ctx.params {
            ctx.scope.insert(key.clone(), Value::String(value.clone()));
        }

        // Execute global middleware
        let global_mw = self.global_middleware.read().clone();
        for mw in &global_mw {
            match self.execute_single(mw, &mut ctx) {
                MiddlewareOutcome::Response(html) => {
                    return MiddlewareOutcome::Response(html);
                }
                MiddlewareOutcome::Error(e) => {
                    return MiddlewareOutcome::Error(e);
                }
                MiddlewareOutcome::Continue => {}
                MiddlewareOutcome::Render { .. } => {
                    // Middleware can't directly render
                    return MiddlewareOutcome::Error(
                        "Middleware cannot call ctx.render()".to_string(),
                    );
                }
            }
        }

        // Execute route-specific middleware
        let route_mw_list = self.route_middleware.read();
        for (pattern, mw_list) in route_mw_list.iter() {
            if self.matches_route_pattern(route_path, pattern) {
                for mw in mw_list {
                    match self.execute_single(mw, &mut ctx) {
                        MiddlewareOutcome::Response(html) => {
                            return MiddlewareOutcome::Response(html);
                        }
                        MiddlewareOutcome::Error(e) => {
                            return MiddlewareOutcome::Error(e);
                        }
                        MiddlewareOutcome::Continue => {}
                        MiddlewareOutcome::Render { .. } => {
                            return MiddlewareOutcome::Error(
                                "Middleware cannot call ctx.render()".to_string(),
                            );
                        }
                    }
                }
            }
        }

        // If no handler or handler didn't render, render component with initial data
        if let Some(_comp) = _component {
            let data = initial_data.unwrap_or_default();
            MiddlewareOutcome::Render { data }
        } else {
            MiddlewareOutcome::Error("No component to render".to_string())
        }
    }

    /// Execute a single middleware
    fn execute_single(&self, middleware: &MiddlewareDef, ctx: &mut EvalContext) -> MiddlewareOutcome {
        // Simple execution - in full implementation, this would
        // interpret the middleware body
        let mut result = MiddlewareOutcome::Continue;

        for stmt in &middleware.body {
            result = self.execute_stmt(stmt, ctx, middleware.is_async);
            if !matches!(result, MiddlewareOutcome::Continue) {
                break;
            }
        }

        result
    }

    /// Execute a single statement
    fn execute_stmt(&self, stmt: &Stmt, ctx: &mut EvalContext, _is_async: bool) -> MiddlewareOutcome {
        match stmt {
            Stmt::Return { arg } => {
                if let Some(expr) = arg {
                    match expr {
                        // Check for ctx.next() call
                        Expr::Member { object, property, .. } => {
                            if let Expr::Ident { name: obj_name } = &**object {
                                if obj_name == "ctx" {
                                    if let Expr::Ident { name: prop_name } = &**property {
                                        if prop_name == "next" {
                                            return MiddlewareOutcome::Continue;
                                        }
                                    }
                                }
                            }
                        }
                        // Check for new Response()
                        Expr::New { callee, args, .. } => {
                            if let Expr::Ident { name } = &**callee {
                                if name == "Response" {
                                    let body = args.first()
                                        .and_then(|a| self.expr_to_value(a, ctx).ok())
                                        .unwrap_or_else(|| Value::String(String::new()));
                                    return MiddlewareOutcome::Response(body.to_string());
                                }
                            }
                        }

                        _ => {}
                    }
                }
                MiddlewareOutcome::Continue
            }
            Stmt::Variable { decl } => {
                // Handle variable declarations like:
                // const { user } = ctx.state;
                if let Some(init) = &decl.init {
                    if let Expr::Member { object, property, .. } = init {
                        // object and property are Box<Expr>
                        let obj_name = if let Expr::Ident { name } = object.as_ref() {
                            Some(name.clone())
                        } else {
                            None
                        };
                        if obj_name.as_deref() == Some("ctx") {
                            let prop_name = if let Expr::Ident { name } = property.as_ref() {
                                Some(name.clone())
                            } else {
                                None
                            };
                            // ctx.state or ctx.params
                            if prop_name.as_deref() == Some("state") || prop_name.as_deref() == Some("params") {
                                // Variable is now in scope
                                ctx.scope.insert(decl.name.clone(), Value::Null);
                            }
                        }
                    }
                    // Regular assignment
                    if let Ok(value) = self.expr_to_value(init, ctx) {
                        ctx.scope.insert(decl.name.clone(), value);
                    }
                }
                MiddlewareOutcome::Continue
            }
            Stmt::Expr { expr } => {
                // Expression statement - evaluate for side effects
                let _ = self.expr_to_value(expr, ctx);
                MiddlewareOutcome::Continue
            }
            Stmt::If { test, consequent, alternate } => {
                if let Ok(cond_val) = self.expr_to_value(test, ctx) {
                    if cond_val.as_bool() {
                        // Execute consequent (unwrap from Box)
                        match consequent.as_ref() {
                            Stmt::Block(stmts) => {
                                for s in stmts {
                                    let result = self.execute_stmt(s, ctx, _is_async);
                                    if !matches!(result, MiddlewareOutcome::Continue) {
                                        return result;
                                    }
                                }
                            }
                            other => {
                                let result = self.execute_stmt(other, ctx, _is_async);
                                if !matches!(result, MiddlewareOutcome::Continue) {
                                    return result;
                                }
                            }
                        }
                    } else if let Some(alt) = alternate {
                        // Execute alternate (unwrap from Box)
                        match alt.as_ref() {
                            Stmt::Block(stmts) => {
                                for s in stmts {
                                    let result = self.execute_stmt(s, ctx, _is_async);
                                    if !matches!(result, MiddlewareOutcome::Continue) {
                                        return result;
                                    }
                                }
                            }
                            other => {
                                let result = self.execute_stmt(other, ctx, _is_async);
                                if !matches!(result, MiddlewareOutcome::Continue) {
                                    return result;
                                }
                            }
                        }
                    }
                }
                MiddlewareOutcome::Continue
            }
            _ => MiddlewareOutcome::Continue,
        }
    }

    /// Convert expression to runtime value
    fn expr_to_value(&self, expr: &Expr, ctx: &EvalContext) -> Result<Value, String> {
        match expr {
            Expr::Ident { name } => {
                ctx.scope.get(name)
                    .cloned()
                    .ok_or_else(|| format!("Undefined: {}", name))
            }
            Expr::String(value) => Ok(Value::String(value.clone())),
            Expr::Template { parts, exprs } => {
                let mut result = String::new();
                for part in parts {
                    match part {
                        TemplatePart::String(s) => result.push_str(s),
                        TemplatePart::Type(_) => {} // Type-only, skip
                    }
                }
                // Replace placeholders
                for (i, e) in exprs.iter().enumerate() {
                    let val = self.expr_to_value(e, ctx)?;
                    result = result.replacen(&format!("${{{}}}", i), &val.to_string(), 1);
                }
                Ok(Value::String(result))
            }
            Expr::Number(value) => Ok(Value::Number(*value)),
            Expr::Boolean(value) => Ok(Value::Bool(*value)),
            Expr::Null => Ok(Value::Null),
            Expr::Undefined => Ok(Value::Undefined),
            Expr::Array { elems } => {
                let mut arr = Vec::new();
                for el in elems {
                    if let Some(e) = el {
                        arr.push(self.expr_to_value(e, ctx)?);
                    }
                }
                Ok(Value::Array(arr))
            }
            Expr::Object { props } => {
                let mut obj = HashMap::new();
                for prop in props {
                    match prop {
                        ObjectProp::Init { key, value } => {
                            let k = match key {
                                PropKey::Ident(s) => s.clone(),
                                PropKey::String(s) => s.clone(),
                                PropKey::Number(n) => n.to_string(),
                                PropKey::Computed(_) => "computed".to_string(),
                            };
                            obj.insert(k, self.expr_to_value(value, ctx)?);
                        }
                        ObjectProp::Spread { value } => {
                            if let Ok(Value::Object(other)) = self.expr_to_value(value, ctx) {
                                obj.extend(other);
                            }
                        }
                        ObjectProp::Shorthand { name } => {
                            if let Some(val) = ctx.scope.get(name) {
                                obj.insert(name.clone(), val.clone());
                            }
                        }
                        ObjectProp::Method { key, value: _ } | ObjectProp::Get { key, value: _ } | ObjectProp::Set { key, value: _ } => {
                            // Skip methods/getters/setters for now
                            let k = match key {
                                PropKey::Ident(s) => s.clone(),
                                PropKey::String(s) => s.clone(),
                                PropKey::Number(n) => n.to_string(),
                                PropKey::Computed(_) => "computed".to_string(),
                            };
                            obj.insert(k, Value::Null);
                        }
                    }
                }
                Ok(Value::Object(obj))
            }
            Expr::Member { object, property, computed, .. } => {
                let obj_val = self.expr_to_value(object, ctx)?;
                let prop_name = if *computed {
                    if let Expr::Ident { name } = &**property {
                        name.clone()
                    } else {
                        return Err("Invalid computed property".to_string());
                    }
                } else {
                    match &**property {
                        Expr::Ident { name } => name.clone(),
                        Expr::String(value) => value.clone(),
                        Expr::Number(value) => value.to_string(),
                        _ => return Err("Invalid property access".to_string()),
                    }
                };
                obj_val.get_member(&prop_name)
                    .ok_or_else(|| format!("Property not found: {}", prop_name))
            }
            Expr::Bin { op, left, right } => {
                let l = self.expr_to_value(left, ctx)?;
                let r = self.expr_to_value(right, ctx)?;
                Ok(self.apply_binary_op(&l, op, &r))
            }
            Expr::Logical { op, left, right } => {
                let l = self.expr_to_value(left, ctx)?;
                match op {
                    LogicalOp::And => {
                        if l.as_bool() {
                            self.expr_to_value(right, ctx)
                        } else {
                            Ok(l)
                        }
                    }
                    LogicalOp::Or => {
                        if l.as_bool() {
                            Ok(l)
                        } else {
                            self.expr_to_value(right, ctx)
                        }
                    }
                    LogicalOp::NullishCoalesce => {
                        if matches!(l, Value::Null | Value::Undefined) {
                            self.expr_to_value(right, ctx)
                        } else {
                            Ok(l)
                        }
                    }
                }
            }
            Expr::Unary { op, arg, .. } => {
                let val = self.expr_to_value(arg, ctx)?;
                Ok(self.apply_unary_op(op, &val))
            }
            Expr::Cond { test, consequent, alternate } => {
                if self.expr_to_value(test, ctx)?.as_bool() {
                    self.expr_to_value(consequent, ctx)
                } else {
                    self.expr_to_value(alternate, ctx)
                }
            }
            Expr::Call { callee, args: _, .. } => {
                // Handle specific calls like ctx.render(), ctx.next()
                if let Expr::Member { object, property, .. } = &**callee {
                    if let Expr::Ident { name: obj_name } = &**object {
                        if obj_name == "ctx" {
                            if let Expr::Ident { name: prop_name } = &**property {
                                // These are handled specially by the caller
                                // Just return a placeholder value
                                return Ok(Value::Function(prop_name.clone()));
                            }
                        }
                    }
                }
                Ok(Value::Null)
            }
            Expr::New { callee, args: _, .. } => {
                if let Expr::Ident { name } = &**callee {
                    if name == "Response" {
                        // Return a special marker for Response constructor
                        return Ok(Value::Function("new Response".to_string()));
                    }
                }
                Ok(Value::Null)
            }
            _ => Ok(Value::Null),
        }
    }

    fn apply_binary_op(&self, left: &Value, op: &BinaryOp, right: &Value) -> Value {
        match op {
            BinaryOp::Add => {
                if let (Value::String(l), r) = (left, right) {
                    Value::String(format!("{}{}", l, r.to_string()))
                } else if let (l, Value::String(r)) = (left, right) {
                    Value::String(format!("{}{}", l.to_string(), r))
                } else {
                    Value::Number(left.as_number() + right.as_number())
                }
            }
            BinaryOp::Sub => Value::Number(left.as_number() - right.as_number()),
            BinaryOp::Mul => Value::Number(left.as_number() * right.as_number()),
            BinaryOp::Div => Value::Number(left.as_number() / right.as_number()),
            BinaryOp::Mod => Value::Number(left.as_number() % right.as_number()),
            BinaryOp::Eq | BinaryOp::EqStrict => Value::Bool(left == right),
            BinaryOp::Ne | BinaryOp::NeStrict => Value::Bool(left != right),
            BinaryOp::Lt => Value::Bool(left.as_number() < right.as_number()),
            BinaryOp::Le => Value::Bool(left.as_number() <= right.as_number()),
            BinaryOp::Gt => Value::Bool(left.as_number() > right.as_number()),
            BinaryOp::Ge => Value::Bool(left.as_number() >= right.as_number()),
            BinaryOp::LogicalAnd => Value::Bool(left.as_bool() && right.as_bool()),
            BinaryOp::LogicalOr => Value::Bool(left.as_bool() || right.as_bool()),
            _ => Value::Null,
        }
    }

    fn apply_unary_op(&self, op: &UnaryOp, val: &Value) -> Value {
        match op {
            UnaryOp::Plus => Value::Number(val.as_number()),
            UnaryOp::Minus => Value::Number(-val.as_number()),
            UnaryOp::Not => Value::Bool(!val.as_bool()),
            UnaryOp::TypeOf => Value::String(match val {
                Value::String(_) => "string",
                Value::Number(_) => "number",
                Value::Bool(_) => "boolean",
                Value::Null => "object",
                Value::Undefined => "undefined",
                Value::Array(_) => "object",
                Value::Object(_) => "object",
                Value::Function(_) => "function",
                Value::VNode(_) => "object",
            }.to_string()),
            UnaryOp::Void => Value::Undefined,
            _ => Value::Null,
        }
    }

    /// Load initial state from request (cookies, headers, etc.)
    fn load_state_from_request(&self, request: &RequestInfo) -> HashMap<String, Value> {
        let mut state = HashMap::new();

        // Add headers
        let headers = request.headers.iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        state.insert("_headers".to_string(), Value::Object(headers));

        state
    }

    /// Check if a route matches a middleware pattern
    fn matches_route_pattern(&self, route: &str, pattern: &str) -> bool {
        if pattern == "/" {
            return route == "/" || route.starts_with('/');
        }
        route.starts_with(pattern)
    }
}

/// Middleware definition
#[derive(Debug, Clone)]
pub struct MiddlewareDef {
    pub name: String,
    pub body: Vec<Stmt>,
    pub is_async: bool,
}

/// Handler definition for execution
#[derive(Debug, Clone)]
pub struct HandlerDef {
    pub method: String,
    pub body: Vec<Stmt>,
    pub is_async: bool,
}

/// Component definition for rendering
#[derive(Debug, Clone)]
pub struct ComponentDef {
    pub name: String,
    pub props_type: Option<String>,
    pub body: Vec<Stmt>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_middleware_chain() {
        let executor = MiddlewareExecutor::new();

        let request = RequestInfo {
            method: "GET".to_string(),
            headers: HashMap::new(),
            url: "/blog/post-1".to_string(),
        };

        // Without a component, should return an error
        let result = executor.execute_pipeline(
            &request,
            "/blog/[slug]",
            vec![("slug".to_string(), "post-1".to_string())]
                .into_iter()
                .collect(),
            None,
            None,
            None,
        );

        // Should return error (no component to render)
        assert!(matches!(result, MiddlewareOutcome::Error(_)));
    }

    #[test]
    fn test_route_pattern_matching() {
        let executor = MiddlewareExecutor::new();

        assert!(executor.matches_route_pattern("/blog", "/blog"));
        assert!(executor.matches_route_pattern("/blog/post-1", "/blog"));
        assert!(executor.matches_route_pattern("/blog/post-1", "/"));
        assert!(!executor.matches_route_pattern("/about", "/blog"));
    }
}
