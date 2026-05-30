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
    fn try_codegen_jsx(&self, items: &serde_json::Value) -> Option<String> {
        let items_arr = items.as_array()?;

        for item in items_arr {
            if item.get("type")?.as_str()? != "Decl" {
                continue;
            }
            let decl = item.get("Decl")?;
            if decl.get("kind")?.as_str()? != "Function" {
                continue;
            }

            let name = decl.get("name")?.as_str()?;
            let body = decl.get("body")?;

            if body.is_null() {
                continue;
            }

            // Find JSX expression in the return statement
            if let Some(jsx_expr) = self.find_jsx_in_body(body) {
                let widget_code = self.generate_jsx_widget_code(jsx_expr)?;
                let code = self.wrap_widget_module(name, &widget_code.to_string());
                return Some(code);
            }
        }

        None
    }

    /// Find JSX expression in function body.
    fn find_jsx_in_body(&self, body: &serde_json::Value) -> Option<serde_json::Value> {
        if let Some(block) = body.get("Block") {
            let stmts = block.get("stmts")?.as_array()?;
            for stmt in stmts {
                if let Some(jsx) = self.find_jsx_in_stmt(stmt) {
                    return Some(jsx);
                }
            }
        } else if self.is_jsx_expr(body) {
            return Some(body.clone());
        }
        None
    }

    /// Find JSX in a statement.
    fn find_jsx_in_stmt(&self, stmt: &serde_json::Value) -> Option<serde_json::Value> {
        let kind = stmt.get("kind")?.as_str()?;
        match kind {
            "Return" => {
                let arg = stmt.get("arg")?;
                if self.is_jsx_expr(arg) {
                    return Some(arg.clone());
                }
                self.find_jsx_in_expr(arg)
            }
            "Expr" => {
                let expr = stmt.get("expr")?;
                if self.is_jsx_expr(expr) {
                    return Some(expr.clone());
                }
                self.find_jsx_in_expr(expr)
            }
            "Block" => {
                if let Some(stmts) = stmt.get("stmts").and_then(|s| s.as_array()) {
                    for s in stmts {
                        if let Some(jsx) = self.find_jsx_in_stmt(s) {
                            return Some(jsx);
                        }
                    }
                }
                None
            }
            "If" => {
                if let Some(cons) = stmt.get("consequent") {
                    if let Some(jsx) = self.find_jsx_in_stmt(cons) {
                        return Some(jsx);
                    }
                }
                if let Some(alt) = stmt.get("alternate") {
                    self.find_jsx_in_stmt(alt)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Find JSX in an expression.
    fn find_jsx_in_expr(&self, expr: &serde_json::Value) -> Option<serde_json::Value> {
        let kind = expr.get("kind")?.as_str()?;
        match kind {
            "JSX" => Some(expr.clone()),
            "Cond" => {
                if let Some(cons) = expr.get("consequent") {
                    if let Some(jsx) = self.find_jsx_in_expr(cons) {
                        return Some(jsx);
                    }
                }
                if let Some(alt) = expr.get("alternate") {
                    self.find_jsx_in_expr(alt)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Check if JSON value is a JSX expression.
    fn is_jsx_expr(&self, val: &serde_json::Value) -> bool {
        val.get("opening").is_some() && val.get("children").is_some()
    }

    /// Generate Ratatui widget code from JSX expression JSON.
    fn generate_jsx_widget_code(&self, jsx: serde_json::Value) -> Option<TokenStream> {
        let opening = jsx.get("opening")?;
        let name = self.jsx_name_to_string(opening.get("name")?)?;

        // Extract attributes
        let attrs = self.extract_jsx_attrs(opening.get("attrs")?)?;

        // Extract children
        let children = self.extract_jsx_children(jsx.get("children")?)?;

        // Map JSX tag to Ratatui widget
        Some(self.tag_to_widget(&name, attrs, children))
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
    fn extract_jsx_attrs(&self, attrs: &serde_json::Value) -> Option<Vec<(String, serde_json::Value)>> {
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

    /// Convert a JSX child to JSON value.
    fn jsx_child_to_value(&self, child: &serde_json::Value) -> Option<Option<serde_json::Value>> {
        // Text child (string)
        if let Some(text) = child.as_str() {
            return Some(Some(serde_json::json!({"kind": "Text", "text": text})));
        }

        let kind = child.get("kind")?.as_str()?;
        match kind {
            "Text" => {
                let text = child.get("0")?.as_str()?;
                Some(Some(serde_json::json!({"kind": "Text", "text": text})))
            }
            "JSX" => {
                let jsx_expr = child.get("JSX")?.clone();
                Some(Some(serde_json::json!({"kind": "JSX", "jsx": jsx_expr})))
            }
            "Fragment" => {
                let frag_children = child.get("Fragment")?.get("children")?;
                let children = self.extract_jsx_children(frag_children)?;
                Some(Some(serde_json::json!({"kind": "Fragment", "children": children})))
            }
            "Expr" => {
                // Expression child - keep as-is for later processing
                Some(Some(child.clone()))
            }
            "Spread" => {
                // Spread children - skip for v0.1
                Some(None)
            }
            _ => Some(None),
        }
    }

    /// Map JSX tag to Ratatui widget code.
    fn tag_to_widget(&self, tag: &str, attrs: Vec<(String, serde_json::Value)>, children: Vec<serde_json::Value>) -> TokenStream {
        match tag {
            "text" => self.widget_paragraph(attrs, children),
            "block" => self.widget_block(attrs, children),
            "row" => self.widget_layout("horizontal", attrs, children),
            "col" => self.widget_layout("vertical", attrs, children),
            "paragraph" => self.widget_paragraph(attrs, children),
            _ => {
                // Unknown tag - treat as paragraph with tag name as text
                let tag_str = tag.to_string();
                quote! {
                    ratatui::widgets::Paragraph::new(#tag_str)
                }
            }
        }
    }

    /// Generate Paragraph widget.
    fn widget_paragraph(&self, attrs: Vec<(String, serde_json::Value)>, children: Vec<serde_json::Value>) -> TokenStream {
        // Extract text from children
        let text = self.extract_text_content(&children);
        let text_str = text.unwrap_or_else(|| "".to_string());

        // Check for block wrapping
        let (block_widget, wrapped) = self.extract_block_wrapper(&attrs);

        if wrapped {
            quote! {
                ratatui::widgets::Paragraph::new(#text_str)
                    .block(#block_widget)
            }
        } else {
            quote! {
                ratatui::widgets::Paragraph::new(#text_str)
            }
        }
    }

    /// Generate Block widget.
    fn widget_block(&self, attrs: Vec<(String, serde_json::Value)>, children: Vec<serde_json::Value>) -> TokenStream {
        let mut title = None;
        let mut borders = true;

        for (name, value) in attrs {
            match name.as_str() {
                "title" => {
                    title = self.value_to_string(&value);
                }
                "borders" => {
                    if let Some(b) = value.as_bool() {
                        borders = b;
                    }
                }
                _ => {}
            }
        }

        let title_quote = title.as_ref().map(|t| quote! { .title(#t) });
        let borders_quote = if borders {
            quote! { .borders(ratatui::widgets::Borders::ALL) }
        } else {
            quote! {}
        };

        // Process children into inner widgets
        let children_tokens: Vec<TokenStream> = children
            .iter()
            .filter_map(|c| self.child_to_widget(c).ok())
            .collect();

        if children_tokens.is_empty() {
            quote! {
                ratatui::widgets::Block::default()
                    #title_quote
                    #borders_quote
            }
        } else {
            // Wrap children in a block with inner rendering
            let child_block = if children_tokens.len() == 1 {
                quote! { #(#children_tokens)* }
            } else {
                quote! { #( #children_tokens )* }
            };

            quote! {
                {
                    let block = ratatui::widgets::Block::default()
                        #title_quote
                        #borders_quote;
                    let inner = block.inner(area);
                    frame.render_widget(block, area);
                    #child_block
                }
            }
        }
    }

    /// Generate Layout widget (row/col).
    fn widget_layout(&self, direction: &str, _attrs: Vec<(String, serde_json::Value)>, children: Vec<serde_json::Value>) -> TokenStream {
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

        // Generate renders for each child chunk
        let renders: Vec<TokenStream> = (0..children_tokens.len())
            .map(|i| {
                let child = &children_tokens[i];
                quote! {
                    {
                        let area = chunks[#i];
                        #child
                    }
                }
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
            "Text" => {
                let text = child.get("text").and_then(|t| t.as_str()).unwrap_or("");
                Ok(quote! {
                    frame.render_widget(ratatui::widgets::Paragraph::new(#text), inner);
                })
            }
            "JSX" => {
                let jsx = child.get("jsx").ok_or(())?;
                self.generate_jsx_widget_code(jsx.clone()).ok_or(())
            }
            "Fragment" => {
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
            "Expr" => {
                // Expression - for v0.1, skip
                Err(())
            }
            _ => Err(()),
        }
    }

    /// Extract text content from children.
    fn extract_text_content(&self, children: &[serde_json::Value]) -> Option<String> {
        let mut text = String::new();
        for child in children {
            let kind = child.get("kind")?.as_str()?;
            match kind {
                "Text" => {
                    let t = child.get("text")?.as_str()?;
                    text.push_str(t);
                }
                "Expr" => {
                    // Skip expressions in v0.1
                }
                _ => {}
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
        let mut title = None;
        let mut borders = true;

        for (name, value) in attrs {
            match name.as_str() {
                "title" => {
                    title = self.value_to_string(value);
                }
                "borders" => {
                    if let Some(b) = value.as_bool() {
                        borders = b;
                    }
                }
                _ => {}
            }
        }

        let title_quote = title.as_ref().map(|t| quote! { .title(#t) });
        let borders_quote = if borders {
            quote! { .borders(ratatui::widgets::Borders::ALL) }
        } else {
            quote! {}
        };

        let has_block_attrs = title.is_some() || !borders;

        (
            quote! {
                ratatui::widgets::Block::default()
                    #title_quote
                    #borders_quote
            },
            has_block_attrs,
        )
    }

    /// Convert JSON value to string.
    fn value_to_string(&self, val: &serde_json::Value) -> Option<String> {
        match val {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Object(obj) => {
                if let Some(expr) = obj.get("Expr") {
                    let kind = expr.get("kind")?.as_str()?;
                    match kind {
                        "Ident" => expr.get("name")?.as_str().map(String::from),
                        "String" => expr.get("0")?.as_str().map(String::from),
                        "Number" => expr.get("0")?.as_f64().map(|n| n.to_string()),
                        _ => None,
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Wrap widget code in a module.
    fn wrap_widget_module(&self, name: &str, widget_fn: &str) -> String {
        format!(
            r#"//! Widget component: {name}
//! Generated by runts-ratatui 0.1

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

        // Try to extract JSX and generate Ratatui widget code
        if let Some(items_json) = &hir.items_json {
            if let Some(code) = self.try_codegen_jsx(items_json) {
                return Ok(code);
            }
        }

        // Fall back to stub
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ratatui_plugin() -> RatatuiPlugin {
        RatatuiPlugin
    }

    /// Normalize quote's spacing around :: for test assertions
    fn normalize(s: &str) -> String {
        let s = s.replace(" :: ", "::");
        let s = s.replace(" ::", "::");
        let s = s.replace(":: ", "::");
        s
    }

    #[test]
    fn test_try_codegen_jsx_simple_text() {
        let plugin = ratatui_plugin();
        let items_json = serde_json::json!([
            {
                "type": "Decl",
                "Decl": {
                    "kind": "Function",
                    "name": "Hello",
                    "body": {
                        "Block": {
                            "stmts": [
                                {
                                    "kind": "Return",
                                    "arg": {
                                        "kind": "JSX",
                                        "opening": {
                                            "name": { "Ident": "text" },
                                            "attrs": [],
                                            "self_closing": false
                                        },
                                        "children": [
                                            { "kind": "Text", "0": "Hello World" }
                                        ],
                                        "closing": {
                                            "name": { "Ident": "text" }
                                        }
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        ]);

        let result = plugin.try_codegen_jsx(&items_json);
        assert!(result.is_some(), "Should generate code for text JSX");
        let code = result.unwrap();
        let code = normalize(&code);
        eprintln!("Generated text code:\n{}", code);
        assert!(code.contains("Paragraph::new"), "Should contain Paragraph");
        assert!(code.contains("Hello World"), "Should contain text content");
    }

    #[test]
    fn test_try_codegen_jsx_block_with_title() {
        let plugin = ratatui_plugin();
        let items_json = serde_json::json!([
            {
                "type": "Decl",
                "Decl": {
                    "kind": "Function",
                    "name": "App",
                    "body": {
                        "Block": {
                            "stmts": [
                                {
                                    "kind": "Return",
                                    "arg": {
                                        "kind": "JSX",
                                        "opening": {
                                            "name": { "Ident": "block" },
                                            "attrs": [
                                                { "Attr": { "name": "title", "value": "My App" } }
                                            ],
                                            "self_closing": false
                                        },
                                        "children": [
                                            { "kind": "Text", "0": "Content" }
                                        ],
                                        "closing": {
                                            "name": { "Ident": "block" }
                                        }
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        ]);

        let result = plugin.try_codegen_jsx(&items_json);
        assert!(result.is_some(), "Should generate code for block JSX");
        let code = result.unwrap();
        let code = normalize(&code);
        eprintln!("Generated block code:\n{}", code);
        assert!(code.contains("Block::default"), "Should contain Block");
        assert!(code.contains("title"), "Should contain title");
        assert!(code.contains("My App"), "Should contain title value");
    }

    #[test]
    fn test_try_codegen_jsx_row() {
        let plugin = ratatui_plugin();
        let items_json = serde_json::json!([
            {
                "type": "Decl",
                "Decl": {
                    "kind": "Function",
                    "name": "Layout",
                    "body": {
                        "Block": {
                            "stmts": [
                                {
                                    "kind": "Return",
                                    "arg": {
                                        "kind": "JSX",
                                        "opening": {
                                            "name": { "Ident": "row" },
                                            "attrs": [],
                                            "self_closing": false
                                        },
                                        "children": [
                                            {
                                                "kind": "JSX",
                                                "JSX": {
                                                    "opening": {
                                                        "name": { "Ident": "text" },
                                                        "attrs": [],
                                                        "self_closing": false
                                                    },
                                                    "children": [
                                                        { "kind": "Text", "0": "Left" }
                                                    ],
                                                    "closing": {
                                                        "name": { "Ident": "text" }
                                                    }
                                                }
                                            },
                                            {
                                                "kind": "JSX",
                                                "JSX": {
                                                    "opening": {
                                                        "name": { "Ident": "text" },
                                                        "attrs": [],
                                                        "self_closing": false
                                                    },
                                                    "children": [
                                                        { "kind": "Text", "0": "Right" }
                                                    ],
                                                    "closing": {
                                                        "name": { "Ident": "text" }
                                                    }
                                                }
                                            }
                                        ],
                                        "closing": {
                                            "name": { "Ident": "row" }
                                        }
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        ]);

        let result = plugin.try_codegen_jsx(&items_json);
        assert!(result.is_some(), "Should generate code for row JSX");
        let code = result.unwrap();
        let code = normalize(&code);
        eprintln!("Generated row code:\n{}", code);
        assert!(code.contains("Layout::default"), "Should contain Layout");
        assert!(code.contains("Horizontal"), "Should be horizontal direction");
        assert!(code.contains("Left"), "Should contain left text");
        assert!(code.contains("Right"), "Should contain right text");
    }

    #[test]
    fn test_try_codegen_jsx_no_jsx_returns_none() {
        let plugin = ratatui_plugin();
        let items_json = serde_json::json!([
            {
                "type": "Decl",
                "Decl": {
                    "kind": "Function",
                    "name": "NoJsx",
                    "body": {
                        "Block": {
                            "stmts": [
                                {
                                    "kind": "Return",
                                    "arg": { "kind": "String", "0": "hello" }
                                }
                            ]
                        }
                    }
                }
            }
        ]);

        let result = plugin.try_codegen_jsx(&items_json);
        assert!(result.is_none(), "Should return None for non-JSX functions");
    }

    #[test]
    fn test_try_codegen_jsx_paragraph() {
        let plugin = ratatui_plugin();
        let items_json = serde_json::json!([
            {
                "type": "Decl",
                "Decl": {
                    "kind": "Function",
                    "name": "Para",
                    "body": {
                        "Block": {
                            "stmts": [
                                {
                                    "kind": "Return",
                                    "arg": {
                                        "kind": "JSX",
                                        "opening": {
                                            "name": { "Ident": "paragraph" },
                                            "attrs": [],
                                            "self_closing": false
                                        },
                                        "children": [
                                            { "kind": "Text", "0": "Test paragraph" }
                                        ],
                                        "closing": {
                                            "name": { "Ident": "paragraph" }
                                        }
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        ]);

        let result = plugin.try_codegen_jsx(&items_json);
        assert!(result.is_some(), "Should generate code for paragraph JSX");
        let code = result.unwrap();
        let code = normalize(&code);
        assert!(code.contains("Paragraph::new"), "Should contain Paragraph");
        assert!(code.contains("Test paragraph"), "Should contain text");
    }

    #[test]
    fn test_try_codegen_jsx_col() {
        let plugin = ratatui_plugin();
        let items_json = serde_json::json!([
            {
                "type": "Decl",
                "Decl": {
                    "kind": "Function",
                    "name": "VLayout",
                    "body": {
                        "Block": {
                            "stmts": [
                                {
                                    "kind": "Return",
                                    "arg": {
                                        "kind": "JSX",
                                        "opening": {
                                            "name": { "Ident": "col" },
                                            "attrs": [],
                                            "self_closing": false
                                        },
                                        "children": [
                                            { "kind": "Text", "0": "Top" }
                                        ],
                                        "closing": {
                                            "name": { "Ident": "col" }
                                        }
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        ]);

        let result = plugin.try_codegen_jsx(&items_json);
        assert!(result.is_some(), "Should generate code for col JSX");
        let code = result.unwrap();
        let code = normalize(&code);
        assert!(code.contains("Layout::default"), "Should contain Layout");
        assert!(code.contains("Vertical"), "Should be vertical direction");
    }

    #[test]
    fn test_try_codegen_jsx_nested_block_in_block() {
        let plugin = ratatui_plugin();
        let items_json = serde_json::json!([
            {
                "type": "Decl",
                "Decl": {
                    "kind": "Function",
                    "name": "Nested",
                    "body": {
                        "Block": {
                            "stmts": [
                                {
                                    "kind": "Return",
                                    "arg": {
                                        "kind": "JSX",
                                        "opening": {
                                            "name": { "Ident": "block" },
                                            "attrs": [
                                                { "Attr": { "name": "title", "value": "Outer" } }
                                            ],
                                            "self_closing": false
                                        },
                                        "children": [
                                            {
                                                "kind": "JSX",
                                                "JSX": {
                                                    "opening": {
                                                        "name": { "Ident": "text" },
                                                        "attrs": [],
                                                        "self_closing": false
                                                    },
                                                    "children": [
                                                        { "kind": "Text", "0": "Inner text" }
                                                    ],
                                                    "closing": {
                                                        "name": { "Ident": "text" }
                                                    }
                                                }
                                            }
                                        ],
                                        "closing": {
                                            "name": { "Ident": "block" }
                                        }
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        ]);

        let result = plugin.try_codegen_jsx(&items_json);
        assert!(result.is_some(), "Should generate code for nested JSX");
        let code = result.unwrap();
        eprintln!("Generated nested code:\n{}", code);
        assert!(code.contains("Outer"), "Should contain outer title");
        assert!(code.contains("Inner text"), "Should contain inner text");
    }
}
