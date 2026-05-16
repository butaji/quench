//! # JSX Transpiler
//!
//! Transpiles JSX/TSX syntax to Rust builder patterns (e.g., Ratatui).

use swc_ecma_ast::*;

/// Transpiles JSX to Rust builder patterns.
pub struct JsxTranspiler {
    /// Widget library to target
    library: WidgetLibrary,
}

/// Target widget library.
#[derive(Debug, Clone, Copy)]
pub enum WidgetLibrary {
    /// Ratatui (TUI)
    Ratatui,
    /// Iced (GUI)
    Iced,
    /// Custom
    Custom,
}

impl Default for WidgetLibrary {
    fn default() -> Self {
        Self::Ratatui
    }
}

impl JsxTranspiler {
    /// Create a new JSX transpiler.
    pub fn new(library: WidgetLibrary) -> Self {
        Self { library }
    }

    /// Transpile a JSX element.
    pub fn transpile_element(&self, elem: &JsxElement) -> String {
        let tag = match &elem.opening.name {
            JsxElementName::Ident(i) => i.sym.to_string(),
            _ => return "unimplemented!()".to_string(),
        };

        let children = elem.children.iter()
            .map(|c| self.transpile_child(c))
            .collect::<Vec<_>>()
            .join(", ");

        let attrs = elem.opening.attrs.iter()
            .map(|a| self.transpile_attr(a))
            .collect::<Vec<_>>()
            .join(", ");

        match self.library {
            WidgetLibrary::Ratatui => self.to_ratatui(&tag, &attrs, &children),
            WidgetLibrary::Iced => self.to_iced(&tag, &attrs, &children),
            WidgetLibrary::Custom => format!("{}::new({})", self.pascal_case(&tag), children),
        }
    }

    /// Transpile a JSX attribute.
    fn transpile_attr(&self, attr: &JsxAttr) -> String {
        let name = match &attr.name {
            JsxAttrName::Ident(i) => i.sym.to_string(),
            _ => return String::new(),
        };

        let value = match &attr.value {
            Some(JsxAttrValue::Str(s)) => format!("{:?}", s.value),
            Some(JsxAttrValue::ExprContainer(e)) => {
                if let Expr::JSXEmptyExpr(_) = *e.expr {
                    String::new()
                } else {
                    self.expr_to_string(&e.expr)
                }
            }
            None => String::new(),
        };

        format!(".{}({})", self.snake_case(&name), value)
    }

    /// Transpile a JSX child element.
    fn transpile_child(&self, child: &JsxChild) -> String {
        match child {
            JsxChild::Element(e) => self.transpile_element(e),
            JsxChild::Fragment(f) => {
                f.children.iter()
                    .map(|c| self.transpile_child(c))
                    .collect::<Vec<_>>()
                    .join("; ")
            }
            JsxChild::Expr(e) => {
                if let Expr::JSXEmptyExpr(_) = *e.expr {
                    String::new()
                } else {
                    self.expr_to_string(&e.expr)
                }
            }
            JsxChild::Spread(_) => String::new(),
            JsxChild::Text(t) => format!("{:?}", t.value),
        }
    }

    /// Convert expression to string.
    fn expr_to_string(&self, expr: &Expr) -> String {
        match expr {
            Expr::Lit(l) => match l {
                Lit::Str(s) => s.value.to_string(),
                Lit::Num(n) => n.value.to_string(),
                Lit::Bool(b) => b.value.to_string().to_string(),
                _ => "unimplemented!()".to_string(),
            },
            Expr::Ident(i) => i.sym.to_string(),
            Expr::Tpl(t) => {
                let mut result = String::new();
                for (i, quasi) in t.quasis.iter().enumerate() {
                    result.push_str(&quasi.raw.value);
                    if i < t.exprs.len() {
                        result.push_str("{ }");
                    }
                }
                format!("format!(\"{}\")", result)
            }
            Expr::Array(a) => {
                let elems = a.elems.iter()
                    .map(|e| e.as_ref().map(|e| self.expr_to_string(&e.expr)).unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{}]", elems)
            }
            Expr::Paren(p) => format!("({})", self.expr_to_string(&p.expr)),
            _ => "unimplemented!()".to_string(),
        }
    }

    /// Convert to Ratatui builder pattern.
    fn to_ratatui(&self, tag: &str, attrs: &str, _children: &str) -> String {
        match tag.to_lowercase().as_str() {
            "block" => format!("Block::default(){}", attrs),
            "list" => format!("List::new(vec![]){}", attrs),
            "listitem" => format!("ListItem::new(\"\"){}", attrs),
            "text" => format!("Paragraph::new(\"\"){}", attrs),
            "table" => format!("Table::new(vec![]){}", attrs),
            "row" => format!("Row::new(vec![]){}", attrs),
            "cell" => format!("Cell::from(\"\"){}", attrs),
            "gauge" => format!("Gauge::default(){}", attrs),
            "spinner" => format!("Sparkline::default(){}", attrs),
            "canvas" => format!("Canvas::default(){}", attrs),
            "div" | "container" => format!("Layout::default(){}", attrs),
            "span" => format!("Span::raw(\"\"){}", attrs),
            "line" => format!("Line::default(){}", attrs),
            _ => format!("/* Unknown widget: {} */ Box::new(())", tag),
        }
    }

    /// Convert to Iced builder pattern.
    fn to_iced(&self, tag: &str, attrs: &str, children: &str) -> String {
        match tag.to_lowercase().as_str() {
            "button" => format!("Button::new({})", children),
            "text" => format!("Text::new({})", children),
            "column" => format!("Column::with_children(vec![{}])", children),
            "row" => format!("Row::with_children(vec![{}])", children),
            "scrollable" => format!("Scrollable::new({})", children),
            "slider" => format!("Slider::default(){}", attrs),
            "checkbox" => format!("Checkbox::new({})", children),
            "textinput" => format!("TextInput::new({})", attrs),
            "container" => format!("Container::new({})", children),
            "scrollable" => format!("Scrollable::new({})", children),
            _ => format!("/* Unknown widget: {} */ Container::new(())", tag),
        }
    }

    /// Convert to snake_case.
    fn snake_case(&self, s: &str) -> String {
        let mut result = String::new();
        for (i, c) in s.chars().enumerate() {
            if c.is_uppercase() && i > 0 {
                result.push('_');
            }
            result.push(c.to_lowercase().next().unwrap_or(c));
        }
        result
    }

    /// Convert to PascalCase.
    fn pascal_case(&self, s: &str) -> String {
        let mut result = String::new();
        let mut capitalize_next = true;
        for c in s.chars() {
            if c == '_' || c == '-' || c == ' ' {
                capitalize_next = true;
            } else if capitalize_next {
                result.push(c.to_uppercase().next().unwrap_or(c));
                capitalize_next = false;
            } else {
                result.push(c);
            }
        }
        result
    }
}

impl Default for JsxTranspiler {
    fn default() -> Self {
        Self::new(WidgetLibrary::default())
    }
}
