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
    BorderStyle, Box as InkBox, Color, Newline, Spacer, Text as InkText, VNode,
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
        Self { default_export }
    }

    /// Run the default export and return the VNode.
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
        &self,
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
            Expr::Array { elems } => {
                let mut vals = Vec::new();
                for e in elems {
                    if let Some(e) = e {
                        vals.push(self.eval_expr(e)?);
                    }
                }
                Ok(vals.into_iter().next().unwrap_or(Value::Undefined))
            }
            Expr::Object { .. } => Ok(Value::Undefined),
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
                for child in children {
                    if let Value::VNode(v) = child {
                        text_content.push_str(&vnode_to_string(&v));
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
    cols: u16,
    rows: u16,
) -> Result<String, RuntimeError> {
    let module = crate::transpile::parser::parse_source(source, true)
        .map_err(|e| RuntimeError(format!("parse error: {e:?}")))?;
    let interp = Interpreter::new(&module);
    let vnode = interp.run()?;
    runts_ink::render_to_string(vnode, runts_ink::RenderOptions::new())
        .map_err(|e| RuntimeError(format!("render error: {e:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_text() {
        let src = r#"
export default function App() {
  #[test]
  fn test_ink_aligned() {
      let src = std::fs::read_to_string("examples/ink-aligned/tui/app.tsx").unwrap();
      let result = render_tsx(&src, 80, 24);
      eprintln!("RESULT: {result:?}");
      match result {
          Ok(output) => eprintln!("OUTPUT: {output}"),
          Err(e) => eprintln!("ERROR: {e:?}"),
      }
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
}
