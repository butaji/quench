//! JSX handling code

use crate::codegen::{jsx_element, jsx_expr, jsx_fragment, jsx_text, page_component};

pub struct JsxHandler;

impl JsxHandler {
    pub fn new() -> Self { Self }

    pub fn try_codegen_jsx(&self, items: &serde_json::Value, _hir: &runts_hir::Module) -> Option<String> {
        let items_arr = items.as_array()?;
        for item in items_arr {
            if let Some(code) = self.try_codegen_jsx_item(item) { return Some(code); }
        }
        None
    }

    fn try_codegen_jsx_item(&self, item: &serde_json::Value) -> Option<String> {
        let func_value = item.get("Decl")?.get("Function")?;
        let name = func_value.get("name")?.as_str()?;
        let body = func_value.get("body")?;
        if !self.has_jsx_content(body) { return None; }
        let jsx_expr = self.find_jsx_in_body(body)?;
        let jsx_code = self.generate_jsx_vnode_code(jsx_expr)?;
        let page_ts = page_component(name, jsx_code.parse().ok()?);
        Some(self.wrap_page_module(name, &page_ts.to_string()))
    }

    fn has_jsx_content(&self, body: &serde_json::Value) -> bool {
        if body.is_null() { return false; }
        let body_str = body.to_string();
        body_str.contains("\"opening\"") || body_str.contains("JSX")
    }

    fn find_jsx_in_body(&self, body: &serde_json::Value) -> Option<serde_json::Value> {
        let stmts_opt = body.as_array()
            .or_else(|| body.get("stmts").and_then(|v| v.as_array()))
            .or_else(|| body.get("Block").and_then(|b| b.get("stmts")).and_then(|v| v.as_array()));
        if let Some(stmts) = stmts_opt {
            for stmt in stmts { if let Some(jsx) = self.find_jsx_in_stmt(stmt) { return Some(jsx); } }
        } else if self.is_jsx_expr(body) { return Some(body.clone()); }
        None
    }

    fn find_jsx_in_stmt(&self, stmt: &serde_json::Value) -> Option<serde_json::Value> {
        let obj = stmt.as_object()?;
        let variant = obj.get("kind")?.as_str()?.to_string();
        match variant.as_str() {
            "Return" => self.find_jsx_in_return(stmt),
            "Expr" => self.find_jsx_in_expr_stmt(stmt),
            "Block" => self.find_jsx_in_block(stmt),
            "If" => self.find_jsx_in_if(stmt),
            _ => None,
        }
    }

    fn find_jsx_in_return(&self, inner: &serde_json::Value) -> Option<serde_json::Value> {
        let arg = inner.get("arg")?;
        if self.is_jsx_expr(arg) { return Some(arg.clone()); }
        self.find_jsx_in_expr(arg)
    }

    fn find_jsx_in_expr_stmt(&self, inner: &serde_json::Value) -> Option<serde_json::Value> {
        let expr = inner.get("expr")?;
        if self.is_jsx_expr(expr) { return Some(expr.clone()); }
        self.find_jsx_in_expr(expr)
    }

    fn find_jsx_in_block(&self, inner: &serde_json::Value) -> Option<serde_json::Value> {
        let stmts = inner.get("stmts")?.as_array()?;
        for s in stmts { if let Some(jsx) = self.find_jsx_in_stmt(s) { return Some(jsx); } }
        None
    }

    fn find_jsx_in_if(&self, inner: &serde_json::Value) -> Option<serde_json::Value> {
        if let Some(cons) = inner.get("consequent") { if let Some(jsx) = self.find_jsx_in_stmt(cons) { return Some(jsx); } }
        if let Some(alt) = inner.get("alternate") { return self.find_jsx_in_stmt(alt); }
        None
    }

    fn find_jsx_in_expr(&self, expr: &serde_json::Value) -> Option<serde_json::Value> {
        let obj = expr.as_object()?;
        let variant = obj.get("kind")?.as_str()?.to_string();
        match variant.as_str() {
            "Call" => self.find_jsx_in_call(expr),
            "Conditional" => self.find_jsx_in_conditional(expr),
            "Logical" => self.find_jsx_in_logical(expr),
            _ => self.find_jsx_in_expr_other(expr, &variant),
        }
    }

    fn find_jsx_in_expr_other(&self, expr: &serde_json::Value, variant: &str) -> Option<serde_json::Value> {
        match variant {
            "Block" => self.find_jsx_in_block(expr),
            "JSXFragment" | "JSXElement" => Some(expr.clone()),
            _ => None,
        }
    }

    fn find_jsx_in_call(&self, expr: &serde_json::Value) -> Option<serde_json::Value> {
        let callee = expr.get("callee")?;
        if self.is_jsx_expr(callee) { return Some(callee.clone()); }
        if let Some(args) = expr.get("arguments").and_then(|v| v.as_array()) {
            for arg in args.iter() {
                if let Some(jsx) = self.find_jsx_in_expr(arg) { return Some(jsx); }
            }
        }
        self.find_jsx_in_expr(callee)
    }

    fn find_jsx_in_conditional(&self, expr: &serde_json::Value) -> Option<serde_json::Value> {
        if let Some(consequent) = expr.get("consequent") {
            if let Some(jsx) = self.find_jsx_in_expr(consequent) { return Some(jsx); }
        }
        if let Some(alternate) = expr.get("alternate") { self.find_jsx_in_expr(alternate) } else { None }
    }

    fn find_jsx_in_logical(&self, expr: &serde_json::Value) -> Option<serde_json::Value> {
        if let Some(right) = expr.get("right") { self.find_jsx_in_expr(right) } else { None }
    }

    fn is_jsx_expr(&self, val: &serde_json::Value) -> bool {
        val.get("JSXElement").is_some() || val.get("JSXFragment").is_some() ||
            val.get("opening").is_some() || val.get("tagName").is_some()
    }

    fn generate_jsx_vnode_code(&self, jsx: serde_json::Value) -> Option<String> {
        if let Some(element) = jsx.get("JSXElement").or_else(|| jsx.get("opening")).cloned() {
            return self.generate_element_code(element);
        }
        if let Some(fragment) = jsx.get("JSXFragment").cloned() {
            return self.generate_fragment_code(fragment);
        }
        None
    }

    fn generate_element_code(&self, element: serde_json::Value) -> Option<String> {
        let tag = element.get("tagName")?.as_str()?;
        let children = element.get("children");
        Some(format!("jsx_element(\"{}\", vec![], vec![{}])", tag, self.children_to_code(children)))
    }

    fn generate_fragment_code(&self, fragment: serde_json::Value) -> Option<String> {
        let children = fragment.get("children");
        Some(format!("jsx_fragment(vec![{}])", self.children_to_code(children)))
    }

    fn children_to_code(&self, children: Option<&serde_json::Value>) -> String {
        match children {
            Some(c) if c.is_array() => c.as_array().unwrap().iter().map(|ch| self.child_to_code(ch)).collect::<Vec<_>>().join(", "),
            Some(c) if c.is_string() => format!("jsx_text({})", c),
            _ => String::new(),
        }
    }

    fn child_to_code(&self, child: &serde_json::Value) -> String {
        if let Some(text) = child.as_str() { return format!("jsx_text(\"{}\")", text.replace('"', "\\\"")); }
        if child.get("JSXElement").is_some() || child.get("opening").is_some() { return self.generate_element_code(child.clone()).unwrap_or_default(); }
        if child.get("JSXFragment").is_some() { return self.generate_fragment_code(child.clone()).unwrap_or_default(); }
        if let Some(expr) = child.get("expr") { return format!("jsx_expr({})", expr); }
        "jsx_text(\"\")".to_string()
    }

    fn wrap_page_module(&self, name: &str, page_fn: &str) -> String {
        format!(r#"//! Page component: {name}
//! Generated by runts-fresh 0.1

use runts_lib::runtime::vdom::VNode;

{page_fn}
"#, name = name, page_fn = page_fn)
    }
}
