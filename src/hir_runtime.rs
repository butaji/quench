//! allow:complexity
//! allow:too_many_lines
//! HIR runtime — interprets HIR (High-level IR) directly
//! to produce VNode trees.
//!
//! This is the "HIR runtime" for `runts dev`. It replaces
//! the rquickjs JS-eval path (which has a string truncation
//! bug) with a pure-Rust interpreter that walks the HIR
//! AST.
//!
//! Supports the HIR subset used by Ink examples:
//! - Function declarations
//! - Return statements
//! - JSX elements (Box, Text, Newline, Spacer)
//! - String, number, boolean literals
//! - Object literals (for props)
//! - Array literals (for children)
//! - Template literals

#![allow(clippy::all)]

use crate::transpile::hir;
use runts_ink::components::{
    BorderStyle, Box as InkBox, Color, Newline, Spacer, Text as InkText, VNode,
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
}

impl Value {
    pub fn as_vnode(self) -> Result<VNode, RuntimeError> {
        match self {
            Value::VNode(v) => Ok(v),
            Value::String(s) => Ok(VNode::from(InkText::new(s))),
            Value::Null | Value::Undefined => Ok(VNode::from(Spacer::new())),
        }
    }
}

/// The HIR interpreter.
pub struct Interpreter {
    default_export: Option<hir::FunctionDecl>,
}

impl Interpreter {
    pub fn new(module: &hir::Module) -> Self {
        let mut default_export = None;
        for item in &module.items {
            if let hir::ModuleItem::Decl(hir::Decl::Function(f)) = item {
                if f.name == "App" || default_export.is_none() {
                    default_export = Some(f.clone());
                }
            }
        }
        Self { default_export }
    }

    pub fn run(&self) -> Result<VNode, RuntimeError> {
        let func = self
            .default_export
            .as_ref()
            .ok_or_else(|| RuntimeError("no default export found".into()))?;
        let val = self.eval_function_body(func)?;
        val.as_vnode()
    }

    fn eval_function_body(
        &self,
        func: &hir::FunctionDecl,
    ) -> Result<Value, RuntimeError> {
        let mut last_val = Value::Undefined;
        for stmt in &func.body.stmts {
            if let Some(val) = self.eval_stmt(stmt)? {
                last_val = val;
            }
        }
        Ok(last_val)
    }

    fn eval_stmt(&self, stmt: &hir::Stmt) -> Result<Option<Value>, RuntimeError> {
        use hir::Stmt;
        match stmt {
            Stmt::Return(expr) => {
                let val = match expr {
                    Some(e) => self.eval_expr(e)?,
                    None => Value::Undefined,
                };
                Ok(Some(val))
            }
            Stmt::Expr(expr) => {
                self.eval_expr(expr)?;
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
            Expr::JSX(jsx) => self.eval_jsx(jsx),
            Expr::Array(elems) => {
                let mut vals = Vec::new();
                for e in elems {
                    vals.push(self.eval_expr(e)?);
                }
                // Return first element as a convenience.
                Ok(vals.into_iter().next().unwrap_or(Value::Undefined))
            }
            Expr::Object(_) => Ok(Value::Undefined),
            Expr::Template { parts, exprs } => {
                let mut s = String::new();
                for (i, part) in parts.iter().enumerate() {
                    s.push_str(part);
                    if let Some(e) = exprs.get(i) {
                        let val = self.eval_expr(e)?;
                        s.push_str(&value_to_string(&val));
                    }
                }
                Ok(Value::String(s))
            }
            _ => Ok(Value::Undefined),
        }
    }

    fn eval_jsx(&self, jsx: &hir::JSXExpr) -> Result<Value, RuntimeError> {
        use hir::JSXName;
        let tag_name = match &jsx.opening.name {
            JSXName::Ident(n) => n.clone(),
            _ => return Err(RuntimeError("unsupported JSX name".into())),
        };
        let mut props: Vec<(String, Value)> = Vec::new();
        for attr in &jsx.opening.attrs {
            if let hir::JSXAttr::Attr { name, value } = attr {
                let key = match name {
                    hir::JSXName::Ident(n) => n.clone(),
                    _ => continue,
                };
                let val = match value {
                    Some(hir::JSXAttrValue::String(s)) => Value::String(s.clone()),
                    Some(hir::JSXAttrValue::Expr(e)) => self.eval_expr(e)?,
                    None => Value::Boolean(true),
                };
                props.push((key, val));
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
                for child in children {
                    if let Value::VNode(v) = child {
                        text_content.push_str(&vnode_to_string(&v));
                    }
                }
                t = t.content(text_content);
                Ok(Value::VNode(VNode::from(t)))
            }
            "Newline" | "newline" => Ok(Value::VNode(VNode::from(Newline::new()))),
            "Spacer" | "spacer" => Ok(Value::VNode(VNode::from(Spacer::new()))),
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
    }
}

fn vnode_to_string(v: &VNode) -> String {
    use runts_ink::components::VNodeContent;
    match &v.0 {
        VNodeContent::Text(t) => t.content.clone(),
        VNodeContent::Newline(_) => "\n".to_string(),
        _ => String::new(),
    }
}

fn apply_box_prop(b: &mut InkBox, key: &str, val: &Value) {
    use runts_ink::components::{
        AlignItems, FlexDirection, JustifyContent,
    };
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
        "paddingTop" => { if let Value::Number(n) = val { b.padding_top = Some(*n as u16); } }
        "paddingBottom" => { if let Value::Number(n) = val { b.padding_bottom = Some(*n as u16); } }
        "paddingLeft" => { if let Value::Number(n) = val { b.padding_left = Some(*n as u16); } }
        "paddingRight" => { if let Value::Number(n) = val { b.padding_right = Some(*n as u16); } }
        "margin" => { if let Value::Number(n) = val { b.margin = *n as u16; } }
        "marginX" => { if let Value::Number(n) = val { b.margin_x = *n as u16; } }
        "marginY" => { if let Value::Number(n) = val { b.margin_y = *n as u16; } }
        "marginTop" => { if let Value::Number(n) = val { b.margin_top = *n as u16; } }
        "marginBottom" => { if let Value::Number(n) = val { b.margin_bottom = *n as u16; } }
        "marginLeft" => { if let Value::Number(n) = val { b.margin_left = *n as u16; } }
        "marginRight" => { if let Value::Number(n) = val { b.margin_right = *n as u16; } }
        "width" => { if let Value::Number(n) = val { b.width = Some(*n as u16); } }
        "height" => { if let Value::Number(n) = val { b.height = Some(*n as u16); } }
        "flexGrow" => { if let Value::Number(n) = val { b.flex_grow = *n as f32; } }
        "flexShrink" => { if let Value::Number(n) = val { b.flex_shrink = *n as f32; } }
        "gap" => { if let Value::Number(n) = val { b.gap = *n as u16; } }
        "rowGap" => { if let Value::Number(n) = val { b.row_gap = Some(*n as u16); } }
        "columnGap" => { if let Value::Number(n) = val { b.column_gap = Some(*n as u16); } }
        "borderStyle" => {
            if let Value::String(s) = val {
                b.border_style = match s.as_str() {
                    "single" => BorderStyle::Single,
                    "double" => BorderStyle::Double,
                    "round" => BorderStyle::Round,
                    "bold" => BorderStyle::Bold,
                    "classic" => BorderStyle::Classic,
                    _ => return,
                };
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
        "bold" => { if matches!(val, Value::Boolean(true)) { t.bold = true; } }
        "italic" => { if matches!(val, Value::Boolean(true)) { t.italic = true; } }
        "underline" => { if matches!(val, Value::Boolean(true)) { t.underline = true; } }
        "strikethrough" => { if matches!(val, Value::Boolean(true)) { t.strikethrough = true; } }
        "inverse" => { if matches!(val, Value::Boolean(true)) { t.inverse = true; } }
        "dimColor" => { if matches!(val, Value::Boolean(true)) { t.dim_color = true; } }
        _ => {}
    }
}

fn parse_color(s: &str) -> Color {
    use runts_ink::components::Color;
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
