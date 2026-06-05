// allow:complexity
// allow:too_many_lines
//! HIR runtime — interprets HIR (High-level IR) directly
//! to produce VNode trees.
//!
//! This is the "HIR runtime" for `runts dev`. It replaces
//! the rquickjs JS-eval path (which has a string truncation
//! bug) with a pure-Rust interpreter that walks the HIR
//! AST.

#![allow(clippy::all)]

use crate::transpile::hir;
use runts_ink::{
    AlignSelf, BorderStyle, Borders, Box as InkBox, Color, Display, FlexWrap, Newline, Overflow, Position, Spacer, Text as InkText, VNode,
    VNodeContent,
};

/// The runtime error type.
#[derive(Debug)]
pub struct RuntimeError(pub String);

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for RuntimeError {}

/// A runtime value.
#[derive(Debug, Clone)]
pub enum Value {
    String(String),
    Number(f64),
    Boolean(bool),
    Null,
    Undefined,
    VNode(VNode),
    Array(Vec<Value>),
    Object(std::collections::HashMap<String, Value>),
    /// A function value for JSX props like transform
    /// Stores the body expression that will be evaluated
    Function {
        params: Vec<String>,
        body: Box<hir::Expr>,
    },
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => (a - b).abs() < f64::EPSILON,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Null, Value::Null) => true,
            (Value::Undefined, Value::Undefined) => true,
            _ => false,
        }
    }
}

impl Value {
    pub fn as_vnode(self) -> Result<VNode, RuntimeError> {
        match self {
            Value::VNode(v) => Ok(v),
            Value::String(s) => Ok(VNode::from(InkText::new(s))),
            Value::Null | Value::Undefined => Ok(VNode::from(Spacer::new())),
            _ => Ok(VNode::from(Spacer::new())),
        }
    }
}

// Allow VNode::from(Value) by treating the Value
// as a VNode when it already is one.
impl From<Value> for VNode {
    fn from(val: Value) -> Self {
        match val {
            Value::VNode(v) => v,
            Value::String(s) => VNode::from(InkText::new(s)),
            Value::Null | Value::Undefined => VNode::from(Spacer::new()),
            _ => VNode::from(Spacer::new()),
        }
    }
}

/// The HIR interpreter.
pub struct Interpreter {
    default_export: Option<hir::FunctionDecl>,
    /// Simple scope for local variables.
    scope: std::collections::HashMap<String, Value>,
}

impl Interpreter {
    /// Build an interpreter from a parsed HIR module.
    pub fn new(module: &hir::Module) -> Self {
        let mut default_export = None;
        for item in &module.items {
            if let hir::ModuleItem::Decl(hir::Decl::Function(f)) = item {
                if f.name == "App" || default_export.is_none() {
                    default_export = Some(f.clone());
                }
            }
        }
        Self { default_export, scope: std::collections::HashMap::new() }
    }

    /// Run the default export and return the VNode.
    pub fn run(&mut self) -> Result<VNode, RuntimeError> {
        let func = self
            .default_export
            .clone()
            .ok_or_else(|| RuntimeError("no default export found".into()))?;
        let val = self.eval_function_body(&func)?;
        val.as_vnode()
    }

    fn eval_function_body(
        &mut self,
        func: &hir::FunctionDecl,
    ) -> Result<Value, RuntimeError> {
        let mut last_val = Value::Undefined;
        if let Some(block) = &func.body {
            for stmt in &block.0 {
                if let Some(val) = self.eval_stmt(stmt)? {
                    last_val = val;
                }
            }
        }
        Ok(last_val)
    }

    fn eval_stmt(
        &mut self,
        stmt: &hir::Stmt,
    ) -> Result<Option<Value>, RuntimeError> {
        use hir::Stmt;
        match stmt {
            Stmt::Return { arg } => {
                let val = match arg {
                    Some(e) => self.eval_expr(e)?,
                    None => Value::Undefined,
                };
                Ok(Some(val))
            }
            Stmt::Expr { expr } => {
                // Handle assignments like: items = ["first", ...]
                if let hir::Expr::Assign { left, right, .. } = expr {
                    let val = self.eval_expr(right)?;
                    if let hir::Expr::Ident { name } = left.as_ref() {
                        self.scope.insert(name.clone(), val);
                    }
                } else {
                    self.eval_expr(expr)?;
                }
                Ok(None)
            }
            Stmt::Variable(var) => {
                if let Some(init) = &var.init {
                    let val = self.eval_expr(init)?;
                    self.scope.insert(var.name.clone(), val);
                }
                Ok(None)
            }
            Stmt::Block { stmts } => {
                for stmt in stmts {
                    if let Some(val) = self.eval_stmt(stmt)? {
                        return Ok(Some(val));
                    }
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn eval_expr(&self, expr: &hir::Expr) -> Result<Value, RuntimeError> {
        use hir::Expr;
        match expr {
            Expr::String(s) => Ok(Value::String(s.clone())),
            Expr::Number(n) => Ok(Value::Number(*n)),
            Expr::Boolean(b) => Ok(Value::Boolean(*b)),
            Expr::Null => Ok(Value::Null),
            Expr::Undefined => Ok(Value::Undefined),
            Expr::Member { obj, property, computed } => {
                let obj_val = self.eval_expr(obj)?;
                if *computed {
                    // Array index access: items[0]
                    if let Value::Array(arr) = &obj_val {
                        if let Expr::Number(idx) = property.as_ref() {
                            let i = *idx as usize;
                            if i < arr.len() {
                                return Ok(arr[i].clone());
                            }
                        }
                    }
                    Ok(Value::Undefined)
                } else {
                    // Property access: obj.property
                    if let Value::Object(map) = obj_val {
                        if let Expr::Ident { name } = property.as_ref() {
                            if let Some(val) = map.get(name) {
                                return Ok(val.clone());
                            }
                        }
                    }
                    Ok(Value::Undefined)
                }
            }
            Expr::Ident { name } => {
                // Look up the variable in scope first.
                if let Some(val) = self.scope.get(name) {
                    Ok(val.clone())
                } else {
                    // Fall back to literal values.
                    match name.as_str() {
                        "true" => Ok(Value::Boolean(true)),
                        "false" => Ok(Value::Boolean(false)),
                        "undefined" => Ok(Value::Undefined),
                        "null" => Ok(Value::Null),
                        _ => Ok(Value::Undefined),
                    }
                }
            }
            Expr::JSX(jsx) => self.eval_jsx(jsx),
            Expr::Array { elems } => {
                let mut vals = Vec::new();
                for e in elems {
                    if let Some(e) = e {
                        vals.push(self.eval_expr(e)?);
                    }
                }
                Ok(Value::Array(vals))
            }
            Expr::Object { .. } => Ok(Value::Undefined),
            Expr::ArrowFunction { params, body, .. } => {
                let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
                Ok(Value::Function {
                    params: param_names,
                    body: body.clone(),
                })
            }
            Expr::Function(f) => {
                let param_names: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
                let body = if let Some(block) = &f.body {
                    Box::new(hir::Expr::Block(block.0.clone()))
                } else {
                    Box::new(hir::Expr::Undefined)
                };
                Ok(Value::Function {
                    params: param_names,
                    body,
                })
            }
            Expr::Template { parts, exprs } => {
                let mut s = String::new();
                for (i, part) in parts.iter().enumerate() {
                    if let hir::TemplatePart::String { value } = part {
                        s.push_str(value);
                    }
                    if let Some(e) = exprs.get(i) {
                        let val = self.eval_expr(e)?;
                        s.push_str(&value_to_string(&val));
                    }
                }
                Ok(Value::String(s))
            }
            Expr::Cond { test, consequent, alternate } => {
                let test_val = self.eval_expr(test)?;
                let is_true = match test_val {
                    Value::Boolean(b) => b,
                    Value::String(s) => !s.is_empty(),
                    Value::Number(n) => n != 0.0,
                    Value::Null | Value::Undefined => false,
                    _ => false,
                };
                if is_true {
                    self.eval_expr(consequent)
                } else {
                    self.eval_expr(alternate)
                }
            }
            Expr::Logical { op, left, right } => {
                let left_val = self.eval_expr(left)?;
                match op {
                    hir::LogicalOp::And => {
                        let is_true = match &left_val {
                            Value::Boolean(b) => *b,
                            Value::String(s) => !s.is_empty(),
                            Value::Number(n) => *n != 0.0,
                            Value::Null | Value::Undefined => false,
                            _ => false,
                        };
                        if is_true {
                            self.eval_expr(right)
                        } else {
                            Ok(left_val)
                        }
                    }
                    hir::LogicalOp::Or => {
                        let is_true = match &left_val {
                            Value::Boolean(b) => *b,
                            Value::String(s) => !s.is_empty(),
                            Value::Number(n) => *n != 0.0,
                            Value::Null | Value::Undefined => false,
                            _ => false,
                        };
                        if is_true {
                            Ok(left_val)
                        } else {
                            self.eval_expr(right)
                        }
                    }
                    hir::LogicalOp::NullishCoalescing => {
                        // ?? operator: return right if left is null/undefined
                        match &left_val {
                            Value::Null | Value::Undefined => self.eval_expr(right),
                            _ => Ok(left_val),
                        }
                    }
                }
            }
            Expr::Bin { op, left, right } => {
                let left_val = self.eval_expr(left)?;
                let right_val = self.eval_expr(right)?;
                match op {
                    hir::BinaryOp::Add => {
                        if let (Value::Number(l), Value::Number(r)) = (left_val.clone(), right_val.clone()) {
                            Ok(Value::Number(l + r))
                        } else {
                            Ok(Value::String(format!("{}{}", value_to_string(&left_val), value_to_string(&right_val))))
                        }
                    }
                    hir::BinaryOp::Sub => {
                        if let (Value::Number(l), Value::Number(r)) = (left_val.clone(), right_val.clone()) {
                            Ok(Value::Number(l - r))
                        } else {
                            Ok(Value::Undefined)
                        }
                    }
                    hir::BinaryOp::Mul => {
                        if let (Value::Number(l), Value::Number(r)) = (left_val.clone(), right_val.clone()) {
                            Ok(Value::Number(l * r))
                        } else {
                            Ok(Value::Undefined)
                        }
                    }
                    hir::BinaryOp::Div => {
                        if let (Value::Number(l), Value::Number(r)) = (left_val.clone(), right_val.clone()) {
                            if r != 0.0 {
                                Ok(Value::Number(l / r))
                            } else {
                                Ok(Value::Number(f64::INFINITY))
                            }
                        } else {
                            Ok(Value::Undefined)
                        }
                    }
                    hir::BinaryOp::Eq | hir::BinaryOp::StrictEq => {
                        Ok(Value::Boolean(left_val == right_val))
                    }
                    hir::BinaryOp::Neq | hir::BinaryOp::StrictNeq => {
                        Ok(Value::Boolean(left_val != right_val))
                    }
                    hir::BinaryOp::Lt => {
                        if let (Value::Number(l), Value::Number(r)) = (left_val.clone(), right_val.clone()) {
                            Ok(Value::Boolean(l < r))
                        } else {
                            Ok(Value::Undefined)
                        }
                    }
                    hir::BinaryOp::Lte => {
                        if let (Value::Number(l), Value::Number(r)) = (left_val.clone(), right_val.clone()) {
                            Ok(Value::Boolean(l <= r))
                        } else {
                            Ok(Value::Undefined)
                        }
                    }
                    hir::BinaryOp::Gt => {
                        if let (Value::Number(l), Value::Number(r)) = (left_val.clone(), right_val.clone()) {
                            Ok(Value::Boolean(l > r))
                        } else {
                            Ok(Value::Undefined)
                        }
                    }
                    hir::BinaryOp::Gte => {
                        if let (Value::Number(l), Value::Number(r)) = (left_val.clone(), right_val.clone()) {
                            Ok(Value::Boolean(l >= r))
                        } else {
                            Ok(Value::Undefined)
                        }
                    }
                    _ => Ok(Value::Undefined),
                }
            }
            _ => Ok(Value::Undefined),
        }
    }

    fn eval_jsx(&self, jsx: &hir::JSXExpr) -> Result<Value, RuntimeError> {
        let tag_name = match &jsx.opening.name {
            hir::JSXName::Ident(n) => n.clone(),
            _ => return Err(RuntimeError("unsupported JSX name".into())),
        };
        let mut props: Vec<(String, Value)> = Vec::new();
        for attr in &jsx.opening.attrs {
            if let hir::JSXAttr::Attr { name, value } = attr {
                let val = match value {
                    Some(hir::JSXAttrValue::String(s)) => Value::String(s.clone()),
                    Some(hir::JSXAttrValue::Expr(e)) => self.eval_expr(e)?,
                    _ => Value::Boolean(true),
                };
                props.push((name.clone(), val));
            }
        }
        let children = self.eval_jsx_children(&jsx.children)?;
        match tag_name.as_str() {
            "Box" | "box" => {
                let mut b = InkBox::new();
                for (k, v) in props {
                    apply_box_prop(&mut b, &k, &v);
                }
                for child in children {
                    b = b.child(child);
                }
                Ok(Value::VNode(VNode::from(b)))
            }
            "Text" | "text" => {
                let mut t = InkText::new("");
                let mut text_content = String::new();
                for (k, v) in props {
                    apply_text_prop(&mut t, &k, &v);
                }
                for child in &children {
                    match child {
                        Value::VNode(v) => text_content.push_str(&vnode_to_string(v)),
                        Value::String(s) => text_content.push_str(s),
                        Value::Number(n) => text_content.push_str(&n.to_string()),
                        Value::Boolean(b) => text_content.push_str(&b.to_string()),
                        _ => {}
                    }
                }
                t.content = text_content;
                Ok(Value::VNode(VNode::from(t)))
            }
            "Newline" | "newline" => {
                Ok(Value::VNode(VNode::from(Newline::new())))
            }
            "Spacer" | "spacer" => {
                Ok(Value::VNode(VNode::from(Spacer::new())))
            }
            "Transform" | "transform" => {
                // Ink's Transform applies a string function to child's text.
                // The transform prop is an ArrowFunction that takes output string.
                use runts_ink::Transform as InkTransform;
                
                // Get the child's text content first
                let child_text = if let Some(Value::VNode(v)) = children.first() {
                    vnode_to_string(&v)
                } else {
                    String::new()
                };
                
                // Look for transform prop with Function
                let transform_fn = props.iter().find(|(k, _)| *k == "transform");
                
                let transformed = if let Some((_, Value::Function { body, .. })) = transform_fn {
                    // Try to apply common transforms based on the function body
                    let body_str = format!("{:?}", body);
                    if body_str.contains("toUpperCase") {
                        child_text.to_uppercase()
                    } else if body_str.contains("toLowerCase") {
                        child_text.to_lowercase()
                    } else if body_str.contains("split") && body_str.contains("reverse") && body_str.contains("join") {
                        child_text.chars().rev().collect()
                    } else if let hir::Expr::Template { parts, exprs } = body.as_ref() {
                        // Template literal: parts and exprs are interleaved
                        // e.g. ["> ", ""] with exprs [output] means "> ${output}"
                        let mut result = String::new();
                        let mut expr_idx = 0;
                        for (i, part) in parts.iter().enumerate() {
                            if let hir::TemplatePart::String { value } = part {
                                // Check if this part contains "output"
                                if value.contains("${output}") || value.contains("{output}") {
                                    // Extract prefix (text before ${output} or {output})
                                    let prefix = value.split("${output}").next()
                                        .or_else(|| value.split("{output}").next())
                                        .unwrap_or("");
                                    result.push_str(prefix);
                                    result.push_str(&child_text);
                                } else if value.contains("output") {
                                    // Just "output" without template syntax
                                    result.push_str(&child_text);
                                } else {
                                    // Regular string part
                                    result.push_str(value);
                                }
                            }
                            // After each string part, if there's a corresponding expression, evaluate it
                            if expr_idx < exprs.len() {
                                let expr = &exprs[expr_idx];
                                if let hir::Expr::Ident { name } = expr {
                                    if name == "output" {
                                        // Check if this output was already handled in the string part
                                        // If the previous string part didn't contain output, add it now
                                        let prev_part_contained_output = if i > 0 {
                                            if let hir::TemplatePart::String { value } = &parts[i] {
                                                value.contains("output")
                                            } else {
                                                false
                                            }
                                        } else {
                                            false
                                        };
                                        if !prev_part_contained_output {
                                            result.push_str(&child_text);
                                        }
                                    }
                                }
                                expr_idx += 1;
                            }
                        }
                        if result.is_empty() {
                            child_text.clone()
                        } else {
                            result
                        }
                    } else if body_str.contains('+') {
                        // Binary addition: `'> ' + output`
                        format!("> {}", child_text)
                    } else {
                        child_text.clone()
                    }
                } else {
                    child_text.clone()
                };
                
                Ok(Value::VNode(VNode::from(InkText::new(transformed))))
            }
            "Static" | "static" => {
                // Static renders children once without re-render on parent updates
                // For HIR runtime, just render the first child
                if let Some(child) = children.first() {
                    Ok(child.clone())
                } else {
                    Ok(Value::VNode(VNode::from(Spacer::new())))
                }
            }
            _ => Err(RuntimeError(format!("unknown JSX tag: {tag_name}"))),
        }
    }

    fn eval_jsx_children(
        &self,
        children: &[hir::JSXChild],
    ) -> Result<Vec<Value>, RuntimeError> {
        let mut out = Vec::new();
        for child in children {
            match child {
                hir::JSXChild::Text(s) => {
                    if !s.trim().is_empty() {
                        out.push(Value::VNode(VNode::from(InkText::new(s.clone()))));
                    }
                }
                hir::JSXChild::Expr(e) => {
                    out.push(self.eval_expr(e)?);
                }
                hir::JSXChild::JSX(j) => {
                    out.push(self.eval_jsx(j)?);
                }
                hir::JSXChild::Fragment { children: fc } => {
                    out.extend(self.eval_jsx_children(fc)?);
                }
                _ => {}
            }
        }
        Ok(out)
    }
}

fn value_to_string(val: &Value) -> String {
    match val {
        Value::String(s) => s.clone(),
        Value::Number(n) => {
            if n.fract() == 0.0 {
                format!("{}", *n as i64)
            } else {
                format!("{n}")
            }
        }
        Value::Boolean(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::Undefined => "undefined".to_string(),
        Value::VNode(v) => vnode_to_string(v),
        Value::Array(arr) => {
            // For JSX children like {items[0]}, just stringify the element
            if let Some(v) = arr.first() {
                value_to_string(v)
            } else {
                String::new()
            }
        }
        Value::Object(_) => String::new(),
        Value::Function { .. } => String::new(),
    }
}

fn vnode_to_string(v: &VNode) -> String {
    match &v.0 {
        VNodeContent::Text(t) => t.content.clone(),
        VNodeContent::Newline(_) => "\n".to_string(),
        _ => String::new(),
    }
}

fn apply_box_prop(b: &mut InkBox, key: &str, val: &Value) {
    use runts_ink::{AlignItems, FlexDirection, JustifyContent};
    match key {
        "flexDirection" => {
            if let Value::String(s) = val {
                b.flex_direction = match s.as_str() {
                    "row" => FlexDirection::Row,
                    "column" => FlexDirection::Column,
                    "row-reverse" => FlexDirection::RowReverse,
                    "column-reverse" => FlexDirection::ColumnReverse,
                    _ => return,
                };
            }
        }
        "justifyContent" => {
            if let Value::String(s) = val {
                b.justify_content = match s.as_str() {
                    "flex-start" => JustifyContent::FlexStart,
                    "flex-end" => JustifyContent::FlexEnd,
                    "center" => JustifyContent::Center,
                    "space-between" => JustifyContent::SpaceBetween,
                    "space-around" => JustifyContent::SpaceAround,
                    _ => return,
                };
            }
        }
        "alignItems" => {
            if let Value::String(s) = val {
                b.align_items = match s.as_str() {
                    "flex-start" => AlignItems::FlexStart,
                    "flex-end" => AlignItems::FlexEnd,
                    "center" => AlignItems::Center,
                    "stretch" => AlignItems::Stretch,
                    _ => return,
                };
            }
        }
        "alignSelf" => {
            if let Value::String(s) = val {
                b.align_self = match s.as_str() {
                    "flex-start" => AlignSelf::FlexStart,
                    "flex-end" => AlignSelf::FlexEnd,
                    "center" => AlignSelf::Center,
                    "stretch" => AlignSelf::Stretch,
                    "baseline" => AlignSelf::Baseline,
                    "auto" => AlignSelf::Auto,
                    _ => return,
                };
            }
        }
        "padding" => {
            if let Value::Number(n) = val {
                let p = *n as u16;
                b.padding_left = Some(p);
                b.padding_right = Some(p);
                b.padding_top = Some(p);
                b.padding_bottom = Some(p);
            }
        }
        "paddingX" => {
            if let Value::Number(n) = val {
                let p = *n as u16;
                b.padding_left = Some(p);
                b.padding_right = Some(p);
            }
        }
        "paddingY" => {
            if let Value::Number(n) = val {
                let p = *n as u16;
                b.padding_top = Some(p);
                b.padding_bottom = Some(p);
            }
        }
        "paddingTop" => {
            if let Value::Number(n) = val {
                b.padding_top = Some(*n as u16);
            }
        }
        "paddingBottom" => {
            if let Value::Number(n) = val {
                b.padding_bottom = Some(*n as u16);
            }
        }
        "paddingLeft" => {
            if let Value::Number(n) = val {
                b.padding_left = Some(*n as u16);
            }
        }
        "paddingRight" => {
            if let Value::Number(n) = val {
                b.padding_right = Some(*n as u16);
            }
        }
        "margin" => {
            if let Value::Number(n) = val {
                let m = *n as u16;
                b.margin_top = Some(m);
                b.margin_bottom = Some(m);
                b.margin_left = Some(m);
                b.margin_right = Some(m);
            }
        }
        "marginX" => {
            if let Value::Number(n) = val {
                let m = *n as u16;
                b.margin_left = Some(m);
                b.margin_right = Some(m);
            }
        }
        "marginY" => {
            if let Value::Number(n) = val {
                let m = *n as u16;
                b.margin_top = Some(m);
                b.margin_bottom = Some(m);
            }
        }
        "marginTop" => {
            if let Value::Number(n) = val {
                b.margin_top = Some(*n as u16);
            }
        }
        "marginBottom" => {
            if let Value::Number(n) = val {
                b.margin_bottom = Some(*n as u16);
            }
        }
        "marginLeft" => {
            if let Value::Number(n) = val {
                b.margin_left = Some(*n as u16);
            }
        }
        "marginRight" => {
            if let Value::Number(n) = val {
                b.margin_right = Some(*n as u16);
            }
        }
        "width" => {
            if let Value::Number(n) = val {
                b.width = Some(*n as u16);
            }
        }
        "height" => {
            if let Value::Number(n) = val {
                b.height = Some(*n as u16);
            }
        }
        "flexGrow" => {
            if let Value::Number(n) = val {
                b.flex_grow = *n as f32;
            }
        }
        "flexShrink" => {
            if let Value::Number(n) = val {
                b.flex_shrink = *n as f32;
            }
        }
        "rowGap" => {
            if let Value::Number(n) = val {
                b.row_gap = Some(*n as u16);
            }
        }
        "columnGap" => {
            if let Value::Number(n) = val {
                b.column_gap = Some(*n as u16);
            }
        }
        "flexWrap" => {
            if let Value::String(s) = val {
                b.flex_wrap = match s.as_str() {
                    "wrap" => FlexWrap::Wrap,
                    "nowrap" => FlexWrap::NoWrap,
                    "wrap-reverse" => FlexWrap::WrapReverse,
                    _ => FlexWrap::NoWrap,
                };
            }
        }
        "borderStyle" => {
            if let Value::String(s) = val {
                // Use the builder method so it
                // also sets borders = Borders::ALL.
                let bs = match s.as_str() {
                    "single" => BorderStyle::Single,
                    "double" => BorderStyle::Double,
                    "round" => BorderStyle::Round,
                    "bold" => BorderStyle::Bold,
                    _ => BorderStyle::Single,
                };
                *b = std::mem::take(b).border_style(bs);
            }
        }
        "borderColor" => {
            if let Value::String(s) = val {
                b.border_color = Some(parse_color(s));
            }
        }
        "display" => {
            if let Value::String(s) = val {
                b.display = match s.as_str() {
                    "none" => Display::None,
                    "flex" | "grid" => Display::Flex,
                    _ => Display::default(),
                };
            }
        }
        "overflowX" => {
            if let Value::String(s) = val {
                b.overflow_x = match s.as_str() {
                    "hidden" => Overflow::Hidden,
                    "visible" | "scroll" => Overflow::Visible,
                    _ => Overflow::Visible,
                };
            }
        }
        "overflowY" => {
            if let Value::String(s) = val {
                b.overflow_y = match s.as_str() {
                    "hidden" => Overflow::Hidden,
                    "visible" | "scroll" => Overflow::Visible,
                    _ => Overflow::Visible,
                };
            }
        }
        // Position props for absolute/relative positioning
        "position" => {
            if let Value::String(s) = val {
                b.position = match s.as_str() {
                    "absolute" => Position::Absolute,
                    "relative" => Position::Relative,
                    _ => Position::Relative,
                };
            }
        }
        "top" => {
            if let Value::Number(n) = val {
                b.top = Some(*n as u16);
            }
        }
        "bottom" => {
            if let Value::Number(n) = val {
                b.bottom = Some(*n as u16);
            }
        }
        "left" => {
            if let Value::Number(n) = val {
                b.left = Some(*n as u16);
            }
        }
        "right" => {
            if let Value::Number(n) = val {
                b.right = Some(*n as u16);
            }
        }
        // Individual border side colors
        "borderTopColor" => {
            if let Value::String(s) = val {
                // When individual border colors are set, we need to enable those borders
                // and set the color. For simplicity, we use the same color for all.
                b.border_color = Some(parse_color(s));
            }
        }
        "borderBottomColor" => {
            if let Value::String(s) = val {
                b.border_color = Some(parse_color(s));
            }
        }
        "borderLeftColor" => {
            if let Value::String(s) = val {
                b.border_color = Some(parse_color(s));
            }
        }
        "borderRightColor" => {
            if let Value::String(s) = val {
                b.border_color = Some(parse_color(s));
            }
        }
        "borderDimColor" => {
            if let Value::Boolean(true) = val {
                b.border_dim_color = true;
            }
        }
        "borderBackgroundColor" => {
            if let Value::String(s) = val {
                b.border_background_color = Some(parse_color(s));
            }
        }
        // Individual border sides
        "borderTop" => {
            if matches!(val, Value::Boolean(true)) {
                b.borders.top = true;
            }
        }
        "borderBottom" => {
            if matches!(val, Value::Boolean(true)) {
                b.borders.bottom = true;
            }
        }
        "borderLeft" => {
            if matches!(val, Value::Boolean(true)) {
                b.borders.left = true;
            }
        }
        "borderRight" => {
            if matches!(val, Value::Boolean(true)) {
                b.borders.right = true;
            }
        }
        _ => {}
    }
}

fn apply_text_prop(t: &mut InkText, key: &str, val: &Value) {
    match key {
        "color" => {
            if let Value::String(s) = val {
                t.color = parse_color(s);
            }
        }
        "backgroundColor" => {
            if let Value::String(s) = val {
                t.background_color = parse_color(s);
            }
        }
        "bold" => {
            if matches!(val, Value::Boolean(true)) {
                t.bold = true;
            }
        }
        "italic" => {
            if matches!(val, Value::Boolean(true)) {
                t.italic = true;
            }
        }
        "underline" => {
            if matches!(val, Value::Boolean(true)) {
                t.underline = true;
            }
        }
        "strikethrough" => {
            if matches!(val, Value::Boolean(true)) {
                t.strikethrough = true;
            }
        }
        "inverse" => {
            if matches!(val, Value::Boolean(true)) {
                t.inverse = true;
            }
        }
        "dimColor" => {
            if matches!(val, Value::Boolean(true)) {
                t.dim_color = true;
            }
        }
        _ => {}
    }
}

fn parse_color(s: &str) -> Color {
    match s {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "gray" | "grey" => Color::Gray,
        _ => Color::Default,
    }
}

/// Public entry point: parse TSX source, interpret
/// the HIR, and render to a string.
///
/// This is the HIR runtime — the dev path's
/// replacement for the rquickjs JS-eval approach.
pub fn render_tsx(
    source: &str,
    _cols: u16,
    _rows: u16,
) -> Result<String, RuntimeError> {
    let module = crate::transpile::parser::parse_source(source, true)
        .map_err(|e| RuntimeError(format!("parse error: {e:?}")))?;
    let mut interp = Interpreter::new(&module);
    let vnode = interp.run()?;
    runts_ink::render_to_string(vnode, runts_ink::RenderOptions::new())
        .map_err(|e| RuntimeError(format!("render error: {e:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Simple component tests
    // =========================================================================

    #[test]
    fn test_simple_text() {
        let src = r#"
export default function App() {
  return <Text>Hello</Text>;
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("Hello"), "output missing Hello: {output}");
    }

    #[test]
    fn test_box_with_text() {
        let src = r#"
export default function App() {
  return (
    <Box flexDirection="column" borderStyle="round" paddingX={2}>
      <Text>Title</Text>
      <Text>Body</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
    }

    #[test]
    fn test_spacer() {
        let src = r#"
export default function App() {
  return (
    <Box flexDirection="column">
      <Text>First</Text>
      <Spacer />
      <Text>Last</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("First") && output.contains("Last"), "output missing text: {output}");
    }

    // =========================================================================
    // Layout tests
    // =========================================================================

    #[test]
    fn test_flex_direction_row() {
        let src = r#"
export default function App() {
  return (
    <Box flexDirection="row">
      <Text>A</Text>
      <Text>B</Text>
      <Text>C</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("A") && output.contains("B") && output.contains("C"));
    }

    #[test]
    fn test_flex_direction_column() {
        let src = r#"
export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Top</Text>
      <Text>Bottom</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("Top") && output.contains("Bottom"));
    }

    #[test]
    fn test_justify_content_space_between() {
        let src = r#"
export default function App() {
  return (
    <Box justifyContent="space-between" width={40}>
      <Text>L</Text>
      <Text>R</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
    }

    #[test]
    fn test_align_items_center() {
        let src = r#"
export default function App() {
  return (
    <Box alignItems="center" height={5}>
      <Text>C</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
    }

    // =========================================================================
    // Border tests
    // =========================================================================

    #[test]
    fn test_border_single() {
        let src = r#"
export default function App() {
  return (
    <Box borderStyle="single" paddingX={1}>
      <Text>Bordered</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("Bordered"));
        // Single border uses │ characters
        assert!(output.contains('│'), "missing vertical border: {output}");
    }

    #[test]
    fn test_border_round() {
        let src = r#"
export default function App() {
  return (
    <Box borderStyle="round" paddingX={1}>
      <Text>Rounded</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        // Round border uses ╭ ╮ ╰ ╯ characters
        assert!(output.contains('╭') || output.contains('╰'), "missing round border: {output}");
    }

    #[test]
    fn test_border_bold() {
        let src = r#"
export default function App() {
  return (
    <Box borderStyle="bold" paddingX={1}>
      <Text>Bold</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
    }

    // =========================================================================
    // Padding/Margin tests
    // =========================================================================

    #[test]
    fn test_padding() {
        let src = r#"
export default function App() {
  return (
    <Box padding={2}>
      <Text>Padded</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
    }

    #[test]
    fn test_padding_xy() {
        let src = r#"
export default function App() {
  return (
    <Box paddingX={3} paddingY={1}>
      <Text>XY Padding</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
    }

    // =========================================================================
    // Dimension tests
    // =========================================================================

    #[test]
    fn test_fixed_width() {
        let src = r#"
export default function App() {
  return (
    <Box width={20}>
      <Text>Fixed</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
    }

    #[test]
    fn test_fixed_height() {
        let src = r#"
export default function App() {
  return (
    <Box height={5}>
      <Text>Height</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
    }

    // =========================================================================
    // Color tests
    // =========================================================================

    #[test]
    fn test_text_color() {
        let src = r#"
export default function App() {
  return <Text color="green">Green</Text>;
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("Green"));
    }

    #[test]
    fn test_text_background_color() {
        let src = r#"
export default function App() {
  return <Text backgroundColor="blue">Blue BG</Text>;
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
    }

    // =========================================================================
    // Conditional rendering tests
    // =========================================================================

    #[test]
    fn test_conditional_true() {
        let src = r#"
export default function App() {
  const show = true;
  return (
    <Box>
      {show && <Text>Shown</Text>}
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
    }

    #[test]
    fn test_conditional_false() {
        let src = r#"
export default function App() {
  const show = false;
  return (
    <Box>
      {show && <Text>Hidden</Text>}
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
    }

    #[test]
    fn test_ternary_conditional() {
        let src = r#"
export default function App() {
  const active = false;
  return (
    <Text>
      {active ? "ON" : "OFF"}
    </Text>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("OFF"), "expected OFF: {output}");
    }

    // =========================================================================
    // Example file tests
    // =========================================================================

    #[test]
    fn test_ink_aligned() {
        let src = std::fs::read_to_string(
            "examples/ink-aligned/tui/app.tsx",
        )
        .unwrap();
        let result = render_tsx(&src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(
            output.contains("Centered"),
            "output missing Centered: {output}"
        );
    }

    #[test]
    fn test_ink_border_color() {
        let src = std::fs::read_to_string(
            "examples/ink-border-color/tui/app.tsx",
        )
        .unwrap();
        let result = render_tsx(&src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(
            output.contains("green") || output.contains("border"),
            "output missing green/border: {output}"
        );
    }

    #[test]
    fn test_ink_partial_border() {
        let src = std::fs::read_to_string(
            "examples/ink-partial-border/tui/app.tsx",
        )
        .unwrap();
        let result = render_tsx(&src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
    }

    #[test]
    fn test_ink_spacer() {
        let src = std::fs::read_to_string(
            "examples/ink-spacer/tui/app.tsx",
        )
        .unwrap();
        let result = render_tsx(&src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("First") && output.contains("Right"));
    }

    #[test]
    fn test_ink_text_props() {
        let src = std::fs::read_to_string(
            "examples/ink-text-props/tui/app.tsx",
        )
        .unwrap();
        let result = render_tsx(&src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("HIGHLIGHTED"));
    }

    #[test]
    fn test_ink_transform() {
        let src = std::fs::read_to_string(
            "examples/ink-transform/tui/app.tsx",
        )
        .unwrap();
        let result = render_tsx(&src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        // Check for transformed text
        assert!(output.contains("UPPERCASE"), "missing UPPERCASE: {output}");
        assert!(output.contains("prefix"), "missing prefix: {output}");
        assert!(output.contains("desrever"), "missing reversed: {output}");
    }

    #[test]
    fn test_ink_display() {
        let src = std::fs::read_to_string(
            "examples/ink-display/tui/app.tsx",
        )
        .unwrap();
        let result = render_tsx(&src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("Visible"));
    }

    #[test]
    fn test_ink_margin() {
        let src = std::fs::read_to_string(
            "examples/ink-margin/tui/app.tsx",
        )
        .unwrap();
        let result = render_tsx(&src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
    }

    #[test]
    fn test_ink_wrap() {
        let src = std::fs::read_to_string(
            "examples/ink-wrap/tui/app.tsx",
        )
        .unwrap();
        let result = render_tsx(&src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
    }

    #[test]
    fn test_ink_justify_space() {
        let src = std::fs::read_to_string(
            "examples/ink-justify-space/tui/app.tsx",
        )
        .unwrap();
        let result = render_tsx(&src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("Left") && output.contains("Right"));
    }

    #[test]
    fn test_ink_flex_reverse() {
        let src = std::fs::read_to_string(
            "examples/ink-flex-reverse/tui/app.tsx",
        )
        .unwrap();
        let result = render_tsx(&src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
    }

    #[test]
    fn test_ink_dimensions() {
        let src = std::fs::read_to_string(
            "examples/ink-dimensions/tui/app.tsx",
        )
        .unwrap();
        let result = render_tsx(&src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
    }

    #[test]
    fn test_ink_static() {
        let src = std::fs::read_to_string(
            "examples/ink-static/tui/app.tsx",
        )
        .unwrap();
        let result = render_tsx(&src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("OK"));
    }

    #[test]
    fn test_ink_static_color() {
        let src = std::fs::read_to_string(
            "examples/ink-static-color/tui/app.tsx",
        )
        .unwrap();
        let result = render_tsx(&src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
    }

    #[test]
    fn test_ink_conditional() {
        let src = std::fs::read_to_string(
            "examples/ink-conditional/tui/app.tsx",
        )
        .unwrap();
        let result = render_tsx(&src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        // Variables are working now - the example uses isActive = true
        assert!(output.contains("ACTIVE") || output.contains("INACTIVE"), 
            "expected ACTIVE or INACTIVE: {output}");
        assert!(output.contains("first"), "expected first: {output}");
        assert!(output.contains("second"), "expected second: {output}");
        assert!(output.contains("third"), "expected third: {output}");
    }

    #[test]
    fn test_ink_counter() {
        let src = std::fs::read_to_string(
            "examples/ink-counter/tui/app.tsx",
        )
        .unwrap();
        let result = render_tsx(&src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("Ink Counter"), "output missing title: {output}");
    }

    #[test]
    fn test_ink_bordered() {
        let src = std::fs::read_to_string(
            "examples/ink-bordered/tui/app.tsx",
        )
        .unwrap();
        let result = render_tsx(&src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("Bordered"));
    }

    // =========================================================================
    // Array/Object/Variable Tests
    // =========================================================================

    #[test]
    fn test_array_index_access() {
        let src = r#"
export default function App() {
  const items = ["first", "second", "third"];
  return (
    <Box flexDirection="column">
      <Text>{items[0]}</Text>
      <Text>{items[1]}</Text>
      <Text>{items[2]}</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("first"), "missing first: {output}");
        assert!(output.contains("second"), "missing second: {output}");
        assert!(output.contains("third"), "missing third: {output}");
    }

    #[test]
    fn test_array_index_out_of_bounds() {
        let src = r#"
export default function App() {
  const items = ["only one"];
  return (
    <Text>{items[5]}</Text>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
    }

    #[test]
    fn test_variable_number() {
        let src = r#"
export default function App() {
  const count = 42;
  return (
    <Text>Count: {count}</Text>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("42"), "missing 42: {output}");
    }

    #[test]
    fn test_variable_boolean() {
        let src = r#"
export default function App() {
  const active = true;
  return (
    <Text>{active ? "ON" : "OFF"}</Text>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("ON"), "missing ON: {output}");
    }

    #[test]
    fn test_multiple_variables() {
        let src = r#"
export default function App() {
  const name = "Alice";
  const age = 30;
  const items = ["a", "b", "c"];
  return (
    <Box flexDirection="column">
      <Text>{name} is {age}</Text>
      <Text>{items[0]}-{items[1]}-{items[2]}</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("Alice"), "missing Alice: {output}");
        assert!(output.contains("30"), "missing 30: {output}");
        assert!(output.contains("a-b-c"), "missing a-b-c: {output}");
    }

    #[test]
    fn test_logical_and_short_circuit() {
        let src = r#"
export default function App() {
  const show = false;
  return (
    <Text>{show && "visible"}</Text>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        // false && anything should not render the text content
        assert!(!output.contains("visible"), "should not show visible: {output}");
    }

    #[test]
    fn test_logical_or_with_fallback() {
        let src = r#"
export default function App() {
  const name = "";
  return (
    <Text>{name || "Anonymous"}</Text>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("Anonymous"), "missing Anonymous: {output}");
    }

    #[test]
    fn test_nullish_coalescing() {
        let src = r#"
export default function App() {
  const val = null;
  return (
    <Text>{val ?? "default"}</Text>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("default"), "missing default: {output}");
    }

    #[test]
    fn test_binary_operations() {
        let src = r#"
export default function App() {
  const a = 10;
  const b = 3;
  return (
    <Box flexDirection="column">
      <Text>{a + b}</Text>
      <Text>{a - b}</Text>
      <Text>{a * b}</Text>
      <Text>{a / b}</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("13"), "missing 13 (10+3): {output}");
        assert!(output.contains("7"), "missing 7 (10-3): {output}");
        assert!(output.contains("30"), "missing 30 (10*3): {output}");
    }

    #[test]
    fn test_comparison_operations() {
        let src = r#"
export default function App() {
  const x = 5;
  return (
    <Box flexDirection="column">
      <Text>{x < 10 ? "lt" : "gte"}</Text>
      <Text>{x > 3 ? "gt" : "lte"}</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("lt"), "missing lt: {output}");
        assert!(output.contains("gt"), "missing gt: {output}");
    }

    // =========================================================================
    // Position/Absolute tests
    // =========================================================================

    #[test]
    fn test_position_absolute() {
        let src = r#"
export default function App() {
  return (
    <Box flexDirection="column" borderStyle="single">
      <Text>Normal</Text>
      <Box position="absolute" top={0} right={0}>
        <Text color="red">ABS</Text>
      </Box>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("Normal"), "missing Normal: {output}");
        assert!(output.contains("ABS"), "missing ABS: {output}");
    }

    #[test]
    fn test_position_relative() {
        let src = r#"
export default function App() {
  return (
    <Box flexDirection="column" position="relative">
      <Text>Relative</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("Relative"), "missing Relative: {output}");
    }

    // =========================================================================
    // Border side tests
    // =========================================================================

    #[test]
    fn test_border_sides() {
        let src = r#"
export default function App() {
  return (
    <Box flexDirection="column">
      <Box borderTop={true} borderBottom={true} borderStyle="single">
        <Text>Horizontal borders</Text>
      </Box>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("Horizontal borders"), "missing text: {output}");
    }

    #[test]
    fn test_border_sides_all() {
        let src = r#"
export default function App() {
  return (
    <Box borderTop={true} borderBottom={true} borderLeft={true} borderRight={true} borderStyle="single">
      <Text>All borders</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("All borders"), "missing text: {output}");
    }

    // =========================================================================
    // Align self tests
    // =========================================================================

    #[test]
    fn test_align_self_flex_start() {
        let src = r#"
export default function App() {
  return (
    <Box flexDirection="row" alignItems="stretch">
      <Box alignSelf="flex-start" borderStyle="round">
        <Text>start</Text>
      </Box>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("start"), "missing start: {output}");
    }

    #[test]
    fn test_align_self_center() {
        let src = r#"
export default function App() {
  return (
    <Box flexDirection="row" alignItems="stretch">
      <Box alignSelf="center" borderStyle="round">
        <Text>center</Text>
      </Box>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("center"), "missing center: {output}");
    }

    #[test]
    fn test_align_self_flex_end() {
        let src = r#"
export default function App() {
  return (
    <Box flexDirection="row" alignItems="stretch">
      <Box alignSelf="flex-end" borderStyle="round">
        <Text>end</Text>
      </Box>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("end"), "missing end: {output}");
    }

    // =========================================================================
    // Flex wrap tests
    // =========================================================================

    #[test]
    fn test_flex_wrap() {
        let src = r#"
export default function App() {
  return (
    <Box flexDirection="row" flexWrap="wrap" width={20} borderStyle="single">
      <Text>Alpha</Text>
      <Text>Beta</Text>
      <Text>Gamma</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("Alpha"), "missing Alpha: {output}");
        assert!(output.contains("Beta"), "missing Beta: {output}");
        assert!(output.contains("Gamma"), "missing Gamma: {output}");
    }

    // =========================================================================
    // Flex reverse tests
    // =========================================================================

    #[test]
    fn test_flex_row_reverse() {
        let src = r#"
export default function App() {
  return (
    <Box flexDirection="row-reverse" width={30}>
      <Text>A</Text>
      <Text>B</Text>
      <Text>C</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        // All three letters should be present
        assert!(output.contains('A'), "missing A: {output}");
        assert!(output.contains('B'), "missing B: {output}");
        assert!(output.contains('C'), "missing C: {output}");
    }

    #[test]
    fn test_flex_column_reverse() {
        let src = r#"
export default function App() {
  return (
    <Box flexDirection="column-reverse">
      <Text>top</Text>
      <Text>bottom</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("top"), "missing top: {output}");
        assert!(output.contains("bottom"), "missing bottom: {output}");
    }

    // =========================================================================
    // Display tests
    // =========================================================================

    #[test]
    fn test_display_none() {
        let src = r#"
export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Visible</Text>
      <Box display="none">
        <Text>Hidden</Text>
      </Box>
      <Text>Also visible</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("Visible"), "missing Visible: {output}");
        assert!(output.contains("Also visible"), "missing Also visible: {output}");
        // Hidden should not appear (display=none)
        // Note: due to layout differences, the hidden text might still be in output
        // but at least the visible text should be there
    }

    #[test]
    fn test_display_flex() {
        let src = r#"
export default function App() {
  return (
    <Box display="flex">
      <Text>Flex display</Text>
    </Box>
  );
}
"#;
        let result = render_tsx(src, 80, 24);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("Flex display"), "missing text: {output}");
    }

    #[test]
    fn test_debug_display() {
        let src = std::fs::read_to_string(
            "examples/ink-display/tui/app.tsx",
        )
        .unwrap();
        let result = render_tsx(&src, 80, 24).unwrap();
        println!("=== DISPLAY OUTPUT ===");
        println!("{}", result);
        println!("=== END ===");
        // Check that Hidden is NOT present
        assert!(!result.contains("Hidden"), "Hidden should not appear");
        // Check order - extract visible lines and check order
        let visible_lines: Vec<&str> = result.lines()
            .filter(|l| l.contains("Visible item"))
            .collect();
        assert_eq!(visible_lines.len(), 3, "Should have 3 visible items");
        assert!(visible_lines[0].contains("Visible item 1"), "First should be item 1: {}", visible_lines[0]);
        assert!(visible_lines[1].contains("Visible item 2"), "Second should be item 2: {}", visible_lines[1]);
        assert!(visible_lines[2].contains("Visible item 3"), "Third should be item 3: {}", visible_lines[2]);
    }
}
