//! Ratatui plugin implementation - real widget codegen from HIR.
//!
//! Parses TSX HIR JSON and converts JSX elements to Ratatui widget code.

use proc_macro2::TokenStream;
use quote::quote;

use runts_plugin::{
    CargoDep, DevAction, DevContext, DevState, Plugin, PluginError,
};

use crate::codegen;

/// Ratatui widget codegen from HIR JSON.
///
/// Maps JSX tags to Ratatui widgets:
/// - `<text>` → `Paragraph::new(...)`
/// - `<block title="..." borders={true}>...</block>` → `Block::default().title(...).borders(...)`
/// - `<row>` / `<col>` → `Layout` with direction
impl RatatuiPlugin {
    /// Try to generate widget code from HIR items JSON.
    /// Returns Some(code) if JSX was detected, None otherwise.
    pub(crate) fn try_codegen_jsx(&self, items: &serde_json::Value) -> Option<String> {
        let items_arr = items.as_array()?;
        for item in items_arr {
            if let Some(jsx_expr) = self.extract_jsx_from_function(item) {
                let widget_code = self.generate_widget_for_jsx(jsx_expr)?;
                let code = self.wrap_widget_module_fn(&widget_code.to_string());
                return Some(code);
            }
        }
        None
    }

    /// Extract JSX from a HIR declaration item.
    fn extract_jsx_from_function(&self, item: &serde_json::Value) -> Option<serde_json::Value> {
        if item.get("type")?.as_str()? != "Decl" {
            return None;
        }
        let decl = item.get("Decl")?;
        if decl.get("kind")?.as_str()? != "Function" {
            return None;
        }
        let body = decl.get("body")?;
        if body.is_null() {
            return None;
        }
        self.find_jsx_in_body(body)
    }

    /// Generate widget code from JSX expression.
    fn generate_widget_for_jsx(&self, jsx: serde_json::Value) -> Option<TokenStream> {
        let opening = jsx.get("opening")?;
        let name = self.jsx_name_to_string(opening.get("name")?)?;
        let attrs = self.extract_jsx_attrs(opening.get("attrs")?)?;
        let children = self.extract_jsx_children(jsx.get("children")?)?;
        Some(self.tag_to_widget(&name, attrs, children))
    }

    /// Find JSX expression in function body.
    fn find_jsx_in_body(&self, body: &serde_json::Value) -> Option<serde_json::Value> {
        if let Some(block) = body.get("Block") {
            let stmts = block.get("stmts")?.as_array()?;
            return self.find_jsx_in_stmts(stmts);
        }
        if self.is_jsx_expr(body) {
            return Some(body.clone());
        }
        None
    }

    /// Find JSX in statement array.
    fn find_jsx_in_stmts(&self, stmts: &[serde_json::Value]) -> Option<serde_json::Value> {
        for stmt in stmts {
            if let Some(jsx) = self.find_jsx_in_stmt(stmt) {
                return Some(jsx);
            }
        }
        None
    }

    /// Find JSX in a statement.
    fn find_jsx_in_stmt(&self, stmt: &serde_json::Value) -> Option<serde_json::Value> {
        let kind = stmt.get("kind")?.as_str()?;
        match kind {
            "Return" => self.find_jsx_in_return(stmt),
            "Expr" => self.find_jsx_in_expr_stmt(stmt),
            "Block" => self.find_jsx_in_block_stmt(stmt),
            "If" => self.find_jsx_in_if_stmt(stmt),
            _ => None,
        }
    }

    /// Find JSX in return statement.
    fn find_jsx_in_return(&self, stmt: &serde_json::Value) -> Option<serde_json::Value> {
        let arg = stmt.get("arg")?;
        if self.is_jsx_expr(arg) {
            return Some(arg.clone());
        }
        self.find_jsx_in_expr(arg)
    }

    /// Find JSX in expression statement.
    fn find_jsx_in_expr_stmt(&self, stmt: &serde_json::Value) -> Option<serde_json::Value> {
        let expr = stmt.get("expr")?;
        if self.is_jsx_expr(expr) {
            return Some(expr.clone());
        }
        self.find_jsx_in_expr(expr)
    }

    /// Find JSX in block statement.
    fn find_jsx_in_block_stmt(&self, stmt: &serde_json::Value) -> Option<serde_json::Value> {
        let stmts = stmt.get("stmts")?.as_array()?;
        self.find_jsx_in_stmts(stmts)
    }

    /// Find JSX in if statement.
    fn find_jsx_in_if_stmt(&self, stmt: &serde_json::Value) -> Option<serde_json::Value> {
        if let Some(cons) = stmt.get("consequent") {
            if let Some(jsx) = self.find_jsx_in_stmt(cons) {
                return Some(jsx);
            }
        }
        if let Some(alt) = stmt.get("alternate") {
            return self.find_jsx_in_stmt(alt);
        }
        None
    }

    /// Find JSX in an expression.
    fn find_jsx_in_expr(&self, expr: &serde_json::Value) -> Option<serde_json::Value> {
        let kind = expr.get("kind")?.as_str()?;
        match kind {
            "JSX" => Some(expr.clone()),
            "Cond" => self.find_jsx_in_cond(expr),
            _ => None,
        }
    }

    /// Find JSX in conditional expression.
    fn find_jsx_in_cond(&self, expr: &serde_json::Value) -> Option<serde_json::Value> {
        if let Some(cons) = expr.get("consequent") {
            if let Some(jsx) = self.find_jsx_in_expr(cons) {
                return Some(jsx);
            }
        }
        if let Some(alt) = expr.get("alternate") {
            return self.find_jsx_in_expr(alt);
        }
        None
    }

    /// Check if JSON value is a JSX expression.
    fn is_jsx_expr(&self, val: &serde_json::Value) -> bool {
        val.get("opening").is_some() && val.get("children").is_some()
    }

    /// Convert JSXName to string.
    fn jsx_name_to_string(&self, name: &serde_json::Value) -> Option<String> {
        match name {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Object(obj) => {
                if let Some(ident) = obj.get("Ident") {
                    return ident.as_str().map(String::from);
                }
                None
            }
            _ => None,
        }
    }

    /// Extract attributes from JSX opening element.
    fn extract_jsx_attrs(
        &self,
        attrs: &serde_json::Value,
    ) -> Option<Vec<(String, serde_json::Value)>> {
        let arr = attrs.as_array()?;
        let mut result = Vec::new();
        for attr in arr {
            if let Some(obj) = attr.get("Attr") {
                let name = obj.get("name")?.as_str()?.to_string();
                let value = obj.get("value")?.clone();
                result.push((name, value));
            }
        }
        Some(result)
    }

    /// Extract children from JSX element.
    fn extract_jsx_children(&self, children: &serde_json::Value) -> Option<Vec<serde_json::Value>> {
        let arr = children.as_array()?;
        let mut result = Vec::new();
        for child in arr {
            if let Some(ts) = self.jsx_child_to_value(child)? {
                result.push(ts);
            }
        }
        Some(result)
    }

    // allow:complexity
    /// Convert a JSX child to JSON value.
    fn jsx_child_to_value(&self, child: &serde_json::Value) -> Option<Option<serde_json::Value>> {
        if child.as_str().is_some() {
            return self.jsx_string_child(child.as_str().unwrap());
        }
        let kind = child.get("kind")?.as_str()?;
        match kind {
            "Text" => self.jsx_text_child(child),
            "JSX" => self.jsx_jsx_child(child),
            "Fragment" => self.jsx_fragment_child(child),
            "Expr" => Some(Some(child.clone())),
            "Spread" => Some(None),
            _ => Some(None),
        }
    }

    /// Handle string child.
    fn jsx_string_child(&self, text: &str) -> Option<Option<serde_json::Value>> {
        Some(Some(serde_json::json!({"kind": "Text", "text": text})))
    }

    /// Handle text child.
    fn jsx_text_child(&self, child: &serde_json::Value) -> Option<Option<serde_json::Value>> {
        let text = child.get("0")?.as_str()?;
        Some(Some(serde_json::json!({"kind": "Text", "text": text})))
    }

    /// Handle JSX child.
    fn jsx_jsx_child(&self, child: &serde_json::Value) -> Option<Option<serde_json::Value>> {
        let jsx_expr = child.get("JSX")?.clone();
        Some(Some(serde_json::json!({"kind": "JSX", "jsx": jsx_expr})))
    }

    /// Handle fragment child.
    fn jsx_fragment_child(&self, child: &serde_json::Value) -> Option<Option<serde_json::Value>> {
        let frag_children = child.get("Fragment")?.get("children")?;
        let children = self.extract_jsx_children(frag_children)?;
        Some(Some(serde_json::json!({"kind": "Fragment", "children": children})))
    }

    /// Map JSX tag to Ratatui widget code.
    fn tag_to_widget(
        &self,
        tag: &str,
        attrs: Vec<(String, serde_json::Value)>,
        children: Vec<serde_json::Value>,
    ) -> TokenStream {
        match tag {
            "text" => self.widget_paragraph(attrs, children),
            "block" => self.widget_block(attrs, children),
            "row" => self.widget_layout("horizontal", attrs, children),
            "col" => self.widget_layout("vertical", attrs, children),
            "paragraph" => self.widget_paragraph(attrs, children),
            _ => {
                let tag_str = tag.to_string();
                quote! { ratatui::widgets::Paragraph::new(#tag_str) }
            }
        }
    }

    /// Generate Paragraph widget.
    fn widget_paragraph(
        &self,
        attrs: Vec<(String, serde_json::Value)>,
        children: Vec<serde_json::Value>,
    ) -> TokenStream {
        let text = self.extract_text_content(&children);
        let text_str = text.unwrap_or_else(|| "".to_string());
        let (block_widget, wrapped) = self.extract_block_wrapper(&attrs);
        if wrapped {
            quote! { ratatui::widgets::Paragraph::new(#text_str).block(#block_widget) }
        } else {
            quote! { ratatui::widgets::Paragraph::new(#text_str) }
        }
    }

    /// Generate Block widget.
    fn widget_block(
        &self,
        attrs: Vec<(String, serde_json::Value)>,
        children: Vec<serde_json::Value>,
    ) -> TokenStream {
        let (title, borders) = self.parse_block_attrs(&attrs);
        let children_tokens = self.build_block_children(&children);
        self.build_block_widget(title, borders, children_tokens)
    }

    /// Parse block attributes.
    fn parse_block_attrs(&self, attrs: &[(String, serde_json::Value)]) -> (Option<String>, bool) {
        let mut title = None;
        let mut borders = true;
        for (name, value) in attrs {
            match name.as_str() {
                "title" => title = self.value_to_string(value),
                "borders" => {
                    if let Some(b) = value.as_bool() {
                        borders = b;
                    }
                }
                _ => {}
            }
        }
        (title, borders)
    }

    /// Build block widget with parsed attributes.
    fn build_block_widget(
        &self,
        title: Option<String>,
        borders: bool,
        children_tokens: Vec<TokenStream>,
    ) -> TokenStream {
        let title_quote = title.as_ref().map(|t| quote! { .title(#t) });
        let borders_quote = if borders {
            quote! { .borders(ratatui::widgets::Borders::ALL) }
        } else {
            quote! {}
        };
        if children_tokens.is_empty() {
            return quote! { ratatui::widgets::Block::default() #title_quote #borders_quote };
        }
        self.render_block_children(children_tokens, title_quote, borders_quote)
    }

    /// Render block with children.
    fn render_block_children(
        &self,
        children_tokens: Vec<TokenStream>,
        title_quote: Option<TokenStream>,
        borders_quote: TokenStream,
    ) -> TokenStream {
        let child_block = if children_tokens.len() == 1 {
            quote! { #(#children_tokens)* }
        } else {
            quote! { #( #children_tokens )* }
        };
        quote! {
            {
                let block = ratatui::widgets::Block::default() #title_quote #borders_quote;
                let inner = block.inner(area);
                frame.render_widget(block, area);
                #child_block
            }
        }
    }

    /// Build token streams from block children.
    fn build_block_children(&self, children: &[serde_json::Value]) -> Vec<TokenStream> {
        children
            .iter()
            .filter_map(|c| self.child_to_widget(c).ok())
            .collect()
    }

    /// Generate Layout widget (row/col).
    fn widget_layout(
        &self,
        direction: &str,
        _attrs: Vec<(String, serde_json::Value)>,
        children: Vec<serde_json::Value>,
    ) -> TokenStream {
        let dir = match direction {
            "horizontal" => quote! { ratatui::layout::Direction::Horizontal },
            _ => quote! { ratatui::layout::Direction::Vertical },
        };
        let child_count = children.len().max(1);
        let constraints: Vec<TokenStream> = (0..child_count)
            .map(|_i| quote! { ratatui::layout::Constraint::Percentage(100 / #child_count as u16) })
            .collect();
        let children_tokens: Vec<TokenStream> = children
            .iter()
            .filter_map(|c| self.child_to_widget(c).ok())
            .collect();
        let renders: Vec<TokenStream> = (0..children_tokens.len())
            .map(|i| {
                let child = &children_tokens[i];
                quote! { { let area = chunks[#i]; #child } }
            })
            .collect();
        quote! {
            {
                let layout = ratatui::layout::Layout::default()
                    .direction(#dir)
                    .constraints(vec![#(#constraints),*]);
                let chunks = layout.split(area);
                #(#renders)*
            }
        }
    }

    /// Convert a child JSON value to widget TokenStream.
    fn child_to_widget(&self, child: &serde_json::Value) -> Result<TokenStream, ()> {
        let kind = child.get("kind").and_then(|k| k.as_str()).ok_or(())?;
        match kind {
            "Text" => self.render_text_child(child),
            "JSX" => self.render_jsx_child(child),
            "Fragment" => self.render_fragment_child(child),
            "Expr" => Err(()),
            _ => Err(()),
        }
    }

    /// Render text child.
    fn render_text_child(&self, child: &serde_json::Value) -> Result<TokenStream, ()> {
        let text = child.get("text").and_then(|t| t.as_str()).unwrap_or("");
        Ok(quote! { frame.render_widget(ratatui::widgets::Paragraph::new(#text), inner); })
    }

    /// Render JSX child.
    fn render_jsx_child(&self, child: &serde_json::Value) -> Result<TokenStream, ()> {
        let jsx = child.get("jsx").ok_or(())?;
        self.generate_widget_for_jsx(jsx.clone()).ok_or(())
    }

    /// Render fragment child.
    fn render_fragment_child(&self, child: &serde_json::Value) -> Result<TokenStream, ()> {
        let children = child.get("children").and_then(|c| c.as_array()).ok_or(())?;
        let tokens: Vec<TokenStream> = children
            .iter()
            .filter_map(|c| self.child_to_widget(c).ok())
            .collect();
        if tokens.len() == 1 {
            Ok(tokens[0].clone())
        } else {
            Ok(quote! { #( #tokens )* })
        }
    }

    /// Extract text content from children.
    fn extract_text_content(&self, children: &[serde_json::Value]) -> Option<String> {
        let mut text = String::new();
        for child in children {
            let kind = child.get("kind")?.as_str()?;
            if kind == "Text" {
                let t = child.get("text")?.as_str()?;
                text.push_str(t);
            }
        }
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    /// Extract block wrapper from attributes if present.
    fn extract_block_wrapper(&self, attrs: &[(String, serde_json::Value)]) -> (TokenStream, bool) {
        let (title, borders) = self.parse_block_attrs(attrs);
        let title_quote = title.as_ref().map(|t| quote! { .title(#t) });
        let borders_quote = if borders {
            quote! { .borders(ratatui::widgets::Borders::ALL) }
        } else {
            quote! {}
        };
        let has_block_attrs = title.is_some() || !borders;
        (quote! { ratatui::widgets::Block::default() #title_quote #borders_quote }, has_block_attrs)
    }

    /// Convert JSON value to string.
    fn value_to_string(&self, val: &serde_json::Value) -> Option<String> {
        match val {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Object(obj) => self.parse_expr_value(obj),
            _ => None,
        }
    }

    // allow:complexity
    /// Parse Expr value from object.
    fn parse_expr_value(&self, obj: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
        let expr = obj.get("Expr")?;
        let kind = expr.get("kind")?.as_str()?;
        let val = expr.get("0")?;
        match kind {
            "Ident" => expr.get("name")?.as_str().map(String::from),
            "String" => val.as_str().map(String::from),
            "Number" => val.as_f64().map(|n| n.to_string()),
            _ => None,
        }
    }

    /// Wrap widget code in a module.
    fn wrap_widget_module_fn(&self, widget_fn: &str) -> String {
        format!(
            r#"//! Widget component: generated by runts-ratatui 0.1

use ratatui::prelude::*;

pub fn render(frame: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect) {{
    let widget = {widget_fn};
    frame.render_widget(widget, area);
}}
"#
        )
    }
}

impl Plugin for RatatuiPlugin {
    fn name(&self) -> &str {
        "ratatui"
    }

    fn help_text(&self) -> &str {
        "Ratatui TUI framework"
    }

    fn codegen_module(&self, hir_str: &str) -> Result<String, PluginError> {
        let hir: runts_plugin::hir::Module = serde_json::from_str(hir_str)
            .map_err(|e| PluginError::codegen("ratatui", "unknown", format!("failed to parse HIR: {e}")))?;
        if let Some(items_json) = &hir.items_json {
            if let Some(code) = self.try_codegen_jsx(items_json) {
                return Ok(code);
            }
        }
        self.codegen_stub()
    }

    fn cargo_deps(&self) -> Vec<CargoDep> {
        vec![
            CargoDep {
                name: "ratatui".to_string(),
                version: Some("0.26".to_string()),
                path: None,
                features: vec!["crossterm".to_string()],
            },
            CargoDep {
                name: "crossterm".to_string(),
                version: Some("0.27".to_string()),
                path: None,
                features: vec![],
            },
            CargoDep {
                name: "anyhow".to_string(),
                version: Some("1.0".to_string()),
                path: None,
                features: vec![],
            },
        ]
    }

    fn codegen_entry(&self, _modules: &[runts_plugin::hir::Module]) -> Result<String, PluginError> {
        let app_body = codegen::widget_text("Hello from Ratatui!");
        let entry = codegen::tui_main(app_body);
        Ok(entry.to_string())
    }

    fn dev_init(&self, _ctx: &mut DevContext) -> Result<Box<dyn DevState>, PluginError> {
        Ok(Box::new(RatatuiDevState))
    }

    fn dev_run_once(&self, _state: &mut dyn DevState) -> Result<DevAction, PluginError> {
        Ok(DevAction::Continue)
    }

    fn dev_reload(&self, _ctx: &mut DevContext, _state: &mut dyn DevState) -> Result<(), PluginError> {
        Ok(())
    }
}

/// Fallback stub when no JSX is detected.
impl RatatuiPlugin {
    fn codegen_stub(&self) -> Result<String, PluginError> {
        let code = quote! {
            use ratatui::prelude::*;

            pub fn render(frame: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect) {
                frame.render_widget(
                    ratatui::widgets::Paragraph::new("Hello from Ratatui!"),
                    area,
                );
            }
        };
        Ok(code.to_string())
    }
}

pub struct RatatuiPlugin;

struct RatatuiDevState;

impl DevState for RatatuiDevState {}