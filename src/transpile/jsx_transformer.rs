//! JSX to html! macro transformer
//!
//! Transforms JSX expressions into Rust code using the html! macro.
//! This is a key part of the transpilation pipeline.

use super::hir::*;

/// JSX transformer that converts JSX to html! macro calls
/// 
/// Note: This transformer provides an alternative approach to JSX transformation
/// using the html! macro. The main codegen embeds JSX generation directly.
#[allow(dead_code)]
pub struct JsxTransformer;

impl JsxTransformer {
    /// Transform a JSX expression into Rust code
    pub fn transform(jsx: &JSXExpr) -> String {
        Self::transform_jsx_element(jsx)
    }
    
    /// Transform a JSX element
    fn transform_jsx_element(jsx: &JSXExpr) -> String {
        let tag = Self::transform_jsx_name(&jsx.opening.name);
        
        // Build attribute string
        let attrs = Self::transform_attrs(&jsx.opening.attrs);
        
        // Build children string
        let children = Self::transform_children(&jsx.children);
        
        if jsx.opening.self_closing {
            // Self-closing: <Tag />
            if attrs.is_empty() {
                format!("html!(<{}/>)", tag)
            } else {
                format!("html!(<{} {} />)", tag, attrs)
            }
        } else {
            // With children: <Tag>children</Tag>
            if attrs.is_empty() {
                format!("html!(<{}>{}</{}>)", tag, children, tag)
            } else {
                format!("html!(<{} {}>{}</{}>)", tag, attrs, children, tag)
            }
        }
    }
    
    /// Transform JSX name (element, component, or member expression)
    fn transform_jsx_name(name: &JSXName) -> String {
        match name {
            JSXName::Ident(s) => {
                // Convert PascalCase to snake_case for components
                // Lowercase for HTML elements
                if s.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                    Self::to_snake_case(s)
                } else {
                    s.clone()
                }
            }
            JSXName::Member { object, property } => {
                format!("{}_{}", 
                    Self::to_snake_case(object),
                    Self::to_snake_case(property)
                )
            }
            JSXName::Namespaced { ns, name } => {
                format!("{}_{}", 
                    Self::to_snake_case(ns),
                    Self::to_snake_case(name)
                )
            }
            JSXName::Dynamic(_) => {
                // Dynamic components need special handling
                "dynamic_component".to_string()
            }
            JSXName::Fragment => {
                // Fragments have no tag name
                String::new()
            }
        }
    }
    
    /// Transform attributes
    fn transform_attrs(attrs: &[JSXAttr]) -> String {
        let mut result = Vec::new();
        
        for attr in attrs {
            match attr {
                JSXAttr::Attr { name, value } => {
                    let rust_name = Self::jsx_attr_to_rust(name);
                    match value {
                        Some(JSXAttrValue::String(s)) => {
                            result.push(format!("{}={:?}", rust_name, s));
                        }
                        Some(JSXAttrValue::Expr(e)) => {
                            let expr = Self::transform_expr(e);
                            result.push(format!("{}={}", rust_name, expr));
                        }
                        None => {
                            result.push(format!("{}: true", rust_name));
                        }
                    }
                }
                JSXAttr::Spread { expr } => {
                    let expr_str = Self::transform_expr(expr);
                    result.push(format!("..{{{}}}", expr_str));
                }
                JSXAttr::Event { name, handler } => {
                    let rust_name = Self::jsx_attr_to_rust(name);
                    let handler_str = Self::transform_expr(handler);
                    result.push(format!("{}={}", rust_name, handler_str));
                }
                JSXAttr::Bool { name } => {
                    let rust_name = Self::jsx_attr_to_rust(name);
                    result.push(format!("{}: true", rust_name));
                }
                JSXAttr::Expr { name, expr } => {
                    // Expression as attribute value
                    let expr_str = Self::transform_expr(expr);
                    if let Some(attr_name) = name {
                        let rust_name = Self::jsx_attr_to_rust(attr_name);
                        result.push(format!("{}={}", rust_name, expr_str));
                    } else {
                        // Fragment-like expression in children position
                        result.push(expr_str);
                    }
                }
            }
        }
        
        result.join(" ")
    }
    
    /// Transform children
    fn transform_children(children: &[JSXChild]) -> String {
        let mut result = Vec::new();
        
        for child in children {
            let child_str = Self::transform_child(child);
            if !child_str.is_empty() {
                result.push(child_str);
            }
        }
        
        result.join(", ")
    }
    
    /// Transform a single child
    fn transform_child(child: &JSXChild) -> String {
        match child {
            JSXChild::Text(s) => {
                // Text content needs escaping and quoting
                format!("{:?}", Self::escape_string(s))
            }
            JSXChild::Expr(e) => {
                Self::transform_expr(e)
            }
            JSXChild::JSX(jsx) => {
                Self::transform_jsx_element(jsx)
            }
            JSXChild::Fragment { children } => {
                // Fragment: <>children</>
                let inner = Self::transform_children(children);
                format!("html!(<>{}</>)", inner)
            }
            JSXChild::Spread { expr } => {
                // Spread children
                let expr_str = Self::transform_expr(expr);
                format!("..{{{}}}", expr_str)
            }
        }
    }
    
    /// Transform an expression
    fn transform_expr(expr: &Expr) -> String {
        match expr {
            Expr::Ident { name } => {
                // Convert identifiers to snake_case
                Self::to_snake_case(name)
            }
            Expr::String(s) => {
                format!("{:?}", s)
            }
            Expr::Number(n) => {
                if n.fract() == 0.0 && *n >= 0.0 && *n <= 255.0 {
                    n.to_string()
                } else {
                    format!("{:?}", n)
                }
            }
            Expr::Boolean(b) => b.to_string(),
            Expr::Null => "None".to_string(),
            Expr::Undefined => "()".to_string(),
            Expr::Template { parts, exprs } => {
                Self::transform_template(parts, exprs)
            }
            Expr::Bin { left, op, right } => {
                let left_str = Self::transform_expr(left);
                let right_str = Self::transform_expr(right);
                let op_str = Self::binop_to_str(op);
                format!("({} {} {})", left_str, op_str, right_str)
            }
            Expr::Unary { op, arg, .. } => {
                let arg_str = Self::transform_expr(arg);
                match op {
                    UnaryOp::Minus => format!("-{}", arg_str),
                    UnaryOp::Plus => format!("+{}", arg_str),
                    UnaryOp::Not => format!("!{}", arg_str),
                    _ => arg_str,
                }
            }
            Expr::Logical { left, op, right } => {
                let left_str = Self::transform_expr(left);
                let right_str = Self::transform_expr(right);
                match op {
                    LogicalOp::And => format!("({} && {})", left_str, right_str),
                    LogicalOp::Or => format!("({} || {})", left_str, right_str),
                    LogicalOp::NullishCoalesce => format!("({}.unwrap_or({}))", left_str, right_str),
                }
            }
            Expr::Cond { test, consequent, alternate } => {
                let test_str = Self::transform_expr(test);
                let cons_str = Self::transform_expr(consequent);
                let alt_str = Self::transform_expr(alternate);
                format!("if {} {{ {} }} else {{ {} }}", test_str, cons_str, alt_str)
            }
            Expr::Call { callee, args, .. } => {
                let callee_str = Self::transform_expr(callee);
                let args_str: Vec<String> = args.iter().map(Self::transform_expr).collect();
                format!("{}({})", callee_str, args_str.join(", "))
            }
            Expr::Member { object, property, computed, .. } => {
                let obj_str = Self::transform_expr(object);
                let prop_str = if *computed {
                    format!("[{}]", Self::transform_expr(property))
                } else {
                    match property.as_ref() {
                        Expr::Ident { name } => format!(".{}", Self::to_snake_case(name)),
                        _ => format!(".({})", Self::transform_expr(property)),
                    }
                };
                format!("{}{}", obj_str, prop_str)
            }
            Expr::Object { props } => {
                Self::transform_object(props)
            }
            Expr::Array { elems } => {
                let elem_strs: Vec<String> = elems.iter()
                    .map(|e| e.as_ref().map_or_else(String::new, Self::transform_expr))
                    .collect();
                format!("vec![{}]", elem_strs.join(", "))
            }
            Expr::Arrow { params, body, .. } => {
                Self::transform_arrow(params, body)
            }
            Expr::Function { decl } => {
                if let Some(body) = &decl.body {
                    Self::transform_arrow(&decl.params, &Stmt::Block(body.0.clone()))
                } else {
                    "|| {}".to_string()
                }
            }
            Expr::Assign { op, left, right } => {
                let left_str = Self::transform_expr(left);
                let right_str = Self::transform_expr(right);
                let op_str = match op {
                    AssignOp::Assign => "=",
                    AssignOp::AddAssign => "+=",
                    AssignOp::SubAssign => "-=",
                    _ => "=",
                };
                format!("{} {} {}", left_str, op_str, right_str)
            }
            Expr::Await { arg } => {
                format!("{}.await", Self::transform_expr(arg))
            }
            Expr::Spread { arg } => {
                format!("..{}", Self::transform_expr(arg))
            }
            _ => "()".to_string(),
        }
    }
    
    /// Transform a template literal
    fn transform_template(parts: &[TemplatePart], exprs: &[Expr]) -> String {
        let mut result = String::new();
        
        for (i, part) in parts.iter().enumerate() {
            match part {
                TemplatePart::String(s) => {
                    result.push_str(&format!("{:?}", s));
                }
                TemplatePart::Type(_) => {}
            }
            
            if i < exprs.len() {
                if !result.is_empty() && !result.ends_with('"') {
                    result.push_str(" + &");
                } else if !result.is_empty() {
                    result.push_str("&");
                }
                result.push_str(&format!("{{{}}}", Self::transform_expr(&exprs[i])));
            }
        }
        
        if result.is_empty() {
            result = "\"\"".to_string();
        }
        
        result
    }
    
    /// Transform an object literal
    fn transform_object(props: &[ObjectProp]) -> String {
        let mut fields = Vec::new();
        
        for prop in props {
            match prop {
                ObjectProp::Init { key, value } => {
                    let k = Self::prop_key_to_string(key);
                    let v = Self::transform_expr(value);
                    fields.push(format!("{}: {}", k, v));
                }
                ObjectProp::Shorthand { name } => {
                    fields.push(Self::to_snake_case(name));
                }
                ObjectProp::Spread { value } => {
                    let v = Self::transform_expr(value);
                    fields.push(format!("..{{{}}}", v));
                }
                ObjectProp::Method { key, value } => {
                    let k = Self::prop_key_to_string(key);
                    let body = value.body.as_ref()
                        .map(|b| Stmt::Block(b.0.clone()))
                        .unwrap_or(Stmt::Empty);
                    let v = Self::transform_arrow(&value.params, &body);
                    fields.push(format!("{}: {}", k, v));
                }
                ObjectProp::Get { key, value } => {
                    let k = Self::prop_key_to_string(key);
                    let body = value.body.as_ref()
                        .map(|b| Stmt::Block(b.0.clone()))
                        .unwrap_or(Stmt::Empty);
                    let v = Self::transform_arrow(&value.params, &body);
                    fields.push(format!("get_{}: {}", Self::to_snake_case(&k), v));
                }
                ObjectProp::Set { key, value } => {
                    let k = Self::prop_key_to_string(key);
                    let param_type = value.params.first().and_then(|p| p.type_.as_ref()).map(|t| Self::type_to_rust(t)).unwrap_or_else(|| "_".to_string());
                    let body = value.body.as_ref()
                        .map(|b| Stmt::Block(b.0.clone()))
                        .unwrap_or(Stmt::Empty);
                    let v = Self::transform_arrow(&value.params, &body);
                    fields.push(format!("set_{}: (|{}: {}| {{ {} }})", Self::to_snake_case(&k), Self::to_snake_case(&value.params.first().map(|p| &p.name).unwrap_or(&"value".to_string())), param_type, v));
                }
            }
        }
        
        format!("{{{}}}", fields.join(", "))
    }
    
    /// Transform an arrow function
    fn transform_arrow(params: &[Param], body: &Stmt) -> String {
        let params_str: Vec<String> = params.iter()
            .map(|p| Self::to_snake_case(&p.name))
            .collect();
        
        let body_str = Self::transform_stmt(body);
        
        if params_str.is_empty() {
            format!("|| {{ {} }}", body_str)
        } else {
            format!("|{}| {{ {} }}", params_str.join(", "), body_str)
        }
    }
    
    /// Transform a statement
    fn transform_stmt(stmt: &Stmt) -> String {
        match stmt {
            Stmt::Empty => String::new(),
            Stmt::Block(stmts) => {
                let inner = stmts.iter().map(Self::transform_stmt).collect::<Vec<_>>().join("; ");
                format!("{{ {} }}", inner)
            }
            Stmt::Return { arg } => {
                if let Some(e) = arg {
                    format!("return {}", Self::transform_expr(e))
                } else {
                    "return".to_string()
                }
            }
            Stmt::Expr { expr } => {
                Self::transform_expr(expr)
            }
            _ => "()".to_string(),
        }
    }
    
    /// Convert property key to string
    fn prop_key_to_string(key: &PropKey) -> String {
        match key {
            PropKey::Ident(s) => Self::to_snake_case(s),
            PropKey::String(s) => format!("{:?}", s),
            PropKey::Number(n) => n.to_string(),
            PropKey::Computed(e) => Self::transform_expr(e),
        }
    }
    
    /// Convert binary operator to string
    fn binop_to_str(op: &BinaryOp) -> &'static str {
        match op {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Mod => "%",
            BinaryOp::Eq | BinaryOp::EqStrict => "==",
            BinaryOp::Ne | BinaryOp::NeStrict => "!=",
            BinaryOp::Lt => "<",
            BinaryOp::Le => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Ge => ">=",
            _ => "+",
        }
    }
    
    /// Convert JSX attribute name to Rust
    fn jsx_attr_to_rust(name: &str) -> String {
        match name {
            // Common attribute mappings
            "class" | "className" => "class_name".to_string(),
            "for" | "htmlFor" => "for_id".to_string(),
            "tabindex" => "tab_index".to_string(),
            "maxlength" => "max_length".to_string(),
            "minlength" => "min_length".to_string(),
            "readonly" => "read_only".to_string(),
            "disabled" => "disabled".to_string(),
            "selected" => "selected".to_string(),
            "checked" => "checked".to_string(),
            "multiple" => "multiple".to_string(),
            "autocomplete" => "autocomplete".to_string(),
            "autofocus" => "autofocus".to_string(),
            "novalidate" => "no_validate".to_string(),
            "crossorigin" => "cross_origin".to_string(),
            "enctype" => "enc_type".to_string(),
            "formaction" => "form_action".to_string(),
            "formenctype" => "form_enc_type".to_string(),
            "formmethod" => "form_method".to_string(),
            "formnovalidate" => "form_no_validate".to_string(),
            "formtarget" => "form_target".to_string(),
            "accesskey" => "access_key".to_string(),
            "contenteditable" => "content_editable".to_string(),
            "contextmenu" => "context_menu".to_string(),
            "dirname" => "dir_name".to_string(),
            "draggable" => "draggable".to_string(),
            "spellcheck" => "spell_check".to_string(),
            "translate" => "translate".to_string(),
            // Event handlers - convert onClick to on_click
            _ if name.starts_with("on") && name.len() > 2 => {
                let event = &name[2..];
                format!("on_{}", Self::to_snake_case(event))
            }
            // data-* attributes - convert to snake_case
            _ if name.starts_with("data-") => {
                format!("data_{}", Self::to_snake_case(&name[5..]))
            }
            // aria-* attributes - convert to snake_case
            _ if name.starts_with("aria-") => {
                format!("aria_{}", Self::to_snake_case(&name[5..]))
            }
            // Default: convert to snake_case
            _ => Self::to_snake_case(name),
        }
    }
    
    /// Convert PascalCase to snake_case
    fn to_snake_case(s: &str) -> String {
        let mut result = String::new();
        for (i, c) in s.chars().enumerate() {
            if c.is_uppercase() && i > 0 {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
        }
        result
    }
    
    /// Escape string for HTML
    fn escape_string(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }
    
    /// Convert TypeScript type to Rust type
    fn type_to_rust(t: &Type) -> String {
        match t {
            Type::String => "String".to_string(),
            Type::Number => "f64".to_string(),
            Type::Boolean => "bool".to_string(),
            Type::Literal { kind, value } => match kind {
                crate::transpile::hir::LiteralKind::String => format!("\"{}\"", value),
                crate::transpile::hir::LiteralKind::Number => value.clone(),
                crate::transpile::hir::LiteralKind::Boolean => value.clone(),
                crate::transpile::hir::LiteralKind::BigInt => format!("{}i64", value),
            },
            Type::Null | Type::Undefined | Type::Void => "()".to_string(),
            Type::Never => "!".to_string(),
            Type::Unknown | Type::Any => "serde_json::Value".to_string(),
            Type::Array { elem } => format!("Vec<{}>", Self::type_to_rust(elem)),
            _ => "serde_json::Value".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_snake_case() {
        assert_eq!(JsxTransformer::to_snake_case("useState"), "use_state");
        assert_eq!(JsxTransformer::to_snake_case("onClick"), "on_click");
        assert_eq!(JsxTransformer::to_snake_case("className"), "class_name");
        assert_eq!(JsxTransformer::to_snake_case("HelloWorld"), "hello_world");
    }
    
    #[test]
    fn test_jsx_attr_to_rust() {
        assert_eq!(JsxTransformer::jsx_attr_to_rust("class"), "class_name");
        assert_eq!(JsxTransformer::jsx_attr_to_rust("className"), "class_name");
        assert_eq!(JsxTransformer::jsx_attr_to_rust("htmlFor"), "for_id");
        assert_eq!(JsxTransformer::jsx_attr_to_rust("onClick"), "on_click");
        assert_eq!(JsxTransformer::jsx_attr_to_rust("dataValue"), "data_value");
        assert_eq!(JsxTransformer::jsx_attr_to_rust("ariaLabel"), "aria_label");
    }
}
