use proc_macro2::TokenStream;

// Re-export app generation functions for lib.rs
pub use crate::codegen::app::{tui_main, widget_block, widget_layout, widget_text};

// Re-export JSX codegen function for plugin.rs
pub(crate) use crate::codegen::jsx::try_codegen_jsx;

// JSX-to-widget codegen helpers
pub(crate) mod app {
    use super::TokenStream;
    use quote::quote;

    /// Generate a block widget with title and borders, rendering children into inner area.
    /// Generates statements that:
    /// 1. Render the Block border to area
    /// 2. Calculate inner area
    /// 3. Render children into inner area
    pub fn widget_block(title: Option<&str>, borders: bool, children: TokenStream) -> TokenStream {
        let title_quote = title.map(|t| quote! { .title(#t) });
        let borders_quote = if borders {
            quote! { .borders(ratatui::widgets::Borders::ALL) }
        } else {
            quote! {}
        };

        quote! {
            {
                let block = ratatui::widgets::Block::default()
                    #title_quote
                    #borders_quote;
                frame.render_widget(block, area);
                let inner = block.inner(area);
                #children
            }
        }
    }

    /// Generate a text/paragraph widget.
    /// Generates a render statement, not a widget expression.
    pub fn widget_text(text: &str) -> TokenStream {
        quote! {
            frame.render_widget(ratatui::widgets::Paragraph::new(#text), area);
        }
    }

    /// Generate a layout widget that splits area and renders children.
    /// Generates statements that:
    /// 1. Create layout with constraints
    /// 2. Split area into chunks
    /// 3. Render each child into its chunk
    pub fn widget_layout(direction: &str, children: Vec<TokenStream>) -> TokenStream {
        let dir = match direction {
            "vertical" => quote! { ratatui::layout::Direction::Vertical },
            "horizontal" => quote! { ratatui::layout::Direction::Horizontal },
            _ => quote! { ratatui::layout::Direction::Vertical },
        };

        let child_count = children.len();
        let constraints: Vec<TokenStream> = (0..child_count)
            .map(|_| quote! { ratatui::layout::Constraint::Percentage(100 / #child_count as u16) })
            .collect();

        // Generate render statements for each child
        let renders: Vec<TokenStream> = children
            .iter()
            .map(|child| {
                quote! {
                    #child
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

    /// Generate panic-safe cleanup using TerminalGuard struct.
    pub fn tui_cleanup() -> TokenStream {
        quote! {
            // Panic-safe terminal cleanup via Drop
            struct TerminalGuard;
            impl Drop for TerminalGuard {
                fn drop(&mut self) {
                    let _ = disable_raw_mode();
                    let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
                }
            }
        }
    }

    /// Generate terminal setup code.
    pub fn tui_setup() -> TokenStream {
        quote! {
            use ratatui::backend::CrosstermBackend;
            use ratatui::Terminal;
            use crossterm::{
                event::{self, Event, KeyCode, KeyEventKind},
                execute,
                terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
            };

            enable_raw_mode()?;
            let mut stdout = std::io::stdout();
            execute!(stdout, EnterAlternateScreen)?;
            let backend = CrosstermBackend::new(stdout);
            let mut terminal = Terminal::new(backend)?;
        }
    }

    // allow:complexity
    pub fn tui_handle_events() -> TokenStream {
        quote! {
            if crossterm::event::poll(std::time::Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => should_quit = true,
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    pub fn tui_loop_body(app_body: TokenStream) -> TokenStream {
        let events = tui_handle_events();
        quote! {
            loop {
                if should_quit { break; }
                terminal.draw(|frame| {
                    let area = frame.size();
                    #app_body
                })?;
                #events
            }
        }
    }

    /// Generate main function for TUI app
    pub fn tui_main(app_body: TokenStream) -> TokenStream {
        let setup = tui_setup();
        let cleanup = tui_cleanup();
        let body = tui_loop_body(app_body);
        quote! {
            fn main() -> anyhow::Result<()> {
                #cleanup
                #setup
                let _guard = TerminalGuard;
                let mut should_quit = false;
                #body
                Ok(())
            }
        }
    }
}

// JSX codegen helpers
pub(crate) mod jsx {
    use super::TokenStream;
    use quote::quote;

    // ============================================================================
    // JSX traversal - finding JSX in HIR statements
    // ============================================================================

    /// Find JSX in a function body.
    pub(crate) fn find_jsx_in_body(body: &serde_json::Value) -> Option<serde_json::Value> {
        if let Some(block) = body.get("Block") {
            let stmts = block.get("stmts")?.as_array()?;
            return find_jsx_in_stmts(stmts);
        }
        if is_jsx_expr(body) {
            return Some(body.clone());
        }
        None
    }

    /// Find JSX in statement array.
    pub(crate) fn find_jsx_in_stmts(stmts: &[serde_json::Value]) -> Option<serde_json::Value> {
        for stmt in stmts {
            if let Some(jsx) = find_jsx_in_stmt(stmt) {
                return Some(jsx);
            }
        }
        None
    }

    /// Find JSX in a statement.
    pub(crate) fn find_jsx_in_stmt(stmt: &serde_json::Value) -> Option<serde_json::Value> {
        let kind = stmt.get("kind")?.as_str()?;
        match kind {
            "Return" => find_jsx_in_return(stmt),
            "Expr" => find_jsx_in_expr_stmt(stmt),
            "Block" => find_jsx_in_block_stmt(stmt),
            "If" => find_jsx_in_if_stmt(stmt),
            _ => None,
        }
    }

    /// Find JSX in return statement.
    fn find_jsx_in_return(stmt: &serde_json::Value) -> Option<serde_json::Value> {
        let arg = stmt.get("arg")?;
        if is_jsx_expr(arg) {
            return Some(arg.clone());
        }
        find_jsx_in_expr(arg)
    }

    /// Find JSX in expression statement.
    fn find_jsx_in_expr_stmt(stmt: &serde_json::Value) -> Option<serde_json::Value> {
        let expr = stmt.get("expr")?;
        if is_jsx_expr(expr) {
            return Some(expr.clone());
        }
        find_jsx_in_expr(expr)
    }

    /// Find JSX in block statement.
    fn find_jsx_in_block_stmt(stmt: &serde_json::Value) -> Option<serde_json::Value> {
        let stmts = stmt.get("stmts")?.as_array()?;
        find_jsx_in_stmts(stmts)
    }

    /// Find JSX in if statement.
    fn find_jsx_in_if_stmt(stmt: &serde_json::Value) -> Option<serde_json::Value> {
        if let Some(cons) = stmt.get("consequent") {
            if let Some(jsx) = find_jsx_in_stmt(cons) {
                return Some(jsx);
            }
        }
        if let Some(alt) = stmt.get("alternate") {
            return find_jsx_in_stmt(alt);
        }
        None
    }

    /// Find JSX in an expression.
    fn find_jsx_in_expr(expr: &serde_json::Value) -> Option<serde_json::Value> {
        let kind = expr.get("kind")?.as_str()?;
        match kind {
            "JSX" => Some(expr.clone()),
            "Cond" => find_jsx_in_cond(expr),
            _ => None,
        }
    }

    /// Find JSX in conditional expression.
    fn find_jsx_in_cond(expr: &serde_json::Value) -> Option<serde_json::Value> {
        if let Some(cons) = expr.get("consequent") {
            if let Some(jsx) = find_jsx_in_expr(cons) {
                return Some(jsx);
            }
        }
        if let Some(alt) = expr.get("alternate") {
            return find_jsx_in_expr(alt);
        }
        None
    }

    /// Check if JSON value is a JSX expression.
    fn is_jsx_expr(val: &serde_json::Value) -> bool {
        val.get("opening").is_some() && val.get("children").is_some()
    }

    // ============================================================================
    // JSX attribute/children extraction
    // ============================================================================

    /// Convert JSXName to string.
    pub(crate) fn jsx_name_to_string(name: &serde_json::Value) -> Option<String> {
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
    pub(crate) fn extract_jsx_attrs(
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
    pub(crate) fn extract_jsx_children(
        children: &serde_json::Value,
    ) -> Option<Vec<serde_json::Value>> {
        let arr = children.as_array()?;
        let mut result = Vec::new();
        for child in arr {
            if let Some(ts) = jsx_child_to_value(child)? {
                result.push(ts);
            }
        }
        Some(result)
    }

    // ============================================================================
    // Child value conversion
    // ============================================================================

    /// Convert a JSX child to JSON value.
    pub(crate) fn jsx_child_to_value(
        child: &serde_json::Value,
    ) -> Option<Option<serde_json::Value>> {
        if child.as_str().is_some() {
            return jsx_string_child(child.as_str().unwrap());
        }
        let kind = child.get("kind")?.as_str()?;
        match kind {
            "Text" => jsx_text_child(child),
            "JSX" => jsx_jsx_child(child),
            "Fragment" => jsx_fragment_child(child),
            "Expr" => Some(Some(child.clone())),
            "Spread" => Some(None),
            _ => Some(None),
        }
    }

    /// Handle string child.
    fn jsx_string_child(text: &str) -> Option<Option<serde_json::Value>> {
        Some(Some(serde_json::json!({"kind": "Text", "text": text})))
    }

    /// Handle text child.
    fn jsx_text_child(child: &serde_json::Value) -> Option<Option<serde_json::Value>> {
        let text = child.get("text")?.as_str()?;
        Some(Some(serde_json::json!({"kind": "Text", "text": text})))
    }

    /// Handle JSX child.
    fn jsx_jsx_child(child: &serde_json::Value) -> Option<Option<serde_json::Value>> {
        let jsx_expr = child.get("JSX")?.clone();
        Some(Some(serde_json::json!({"kind": "JSX", "jsx": jsx_expr})))
    }

    /// Handle fragment child.
    fn jsx_fragment_child(child: &serde_json::Value) -> Option<Option<serde_json::Value>> {
        let frag_children = child.get("Fragment")?.get("children")?;
        let children = extract_jsx_children(frag_children)?;
        Some(Some(serde_json::json!({"kind": "Fragment", "children": children})))
    }

    /// Convert JSON value to string.
    pub(crate) fn value_to_string(val: &serde_json::Value) -> Option<String> {
        match val {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Object(obj) => parse_expr_value(obj),
            _ => None,
        }
    }

    /// Parse Expr value from object.
    pub(crate) fn parse_expr_value(
        obj: &serde_json::Map<String, serde_json::Value>,
    ) -> Option<String> {
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

    // ============================================================================
    // Widget generation (TokenStream building)
    // ============================================================================

    /// Extract JSX from a HIR declaration item.
    pub(crate) fn extract_jsx_from_function(item: &serde_json::Value) -> Option<serde_json::Value> {
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
        find_jsx_in_body(body)
    }

    /// Generate widget code from JSX expression.
    pub(crate) fn generate_widget_for_jsx(jsx: serde_json::Value) -> Option<TokenStream> {
        let opening = jsx.get("opening")?;
        let name = jsx_name_to_string(opening.get("name")?)?;
        let attrs = extract_jsx_attrs(opening.get("attrs")?)?;
        let children = extract_jsx_children(jsx.get("children")?)?;
        Some(tag_to_widget(&name, attrs, children))
    }

    /// Map JSX tag to Ratatui widget code.
    fn tag_to_widget(
        tag: &str,
        attrs: Vec<(String, serde_json::Value)>,
        children: Vec<serde_json::Value>,
    ) -> TokenStream {
        match tag {
            "text" => widget_paragraph(attrs, children),
            "block" => widget_block(attrs, children),
            "row" => widget_layout(attrs, children),
            "col" => widget_layout_vertical(attrs, children),
            "paragraph" => widget_paragraph(attrs, children),
            // Ink-style tags. `Box` is a recursive
            // container; `Text` is an alias for the
            // existing paragraph widget; `Newline` /
            // `Spacer` are layout-only constructs that
            // produce a no-op widget; `Static` /
            // `Transform` fall through to render their
            // children.
            "Box" | "box" => widget_ink_box(attrs, children),
            "Text" | "inktext" => widget_paragraph(attrs, children),
            "Newline" | "newline" => widget_ink_newline(),
            "Spacer" | "spacer" => widget_ink_spacer(),
            "Static" | "static" => widget_ink_first_child(children),
            "Transform" | "transform" => widget_ink_first_child(children),
            _ => {
                let tag_str = tag.to_string();
                quote! { ratatui::widgets::Paragraph::new(#tag_str) }
            }
        }
    }

    /// Generate Paragraph widget.
    fn widget_paragraph(
        attrs: Vec<(String, serde_json::Value)>,
        children: Vec<serde_json::Value>,
    ) -> TokenStream {
        let text = extract_text_content(&children);
        let text_str = text.unwrap_or_else(|| "".to_string());
        let (block_widget, wrapped) = extract_block_wrapper(&attrs);
        if wrapped {
            quote! { ratatui::widgets::Paragraph::new(#text_str).block(#block_widget) }
        } else {
            quote! { ratatui::widgets::Paragraph::new(#text_str) }
        }
    }

    /// Generate Block widget (internal).
    fn widget_block(
        attrs: Vec<(String, serde_json::Value)>,
        children: Vec<serde_json::Value>,
    ) -> TokenStream {
        let (title, borders) = parse_block_attrs(&attrs);
        let children_tokens = build_block_children(&children);
        build_block_widget(title, borders, children_tokens)
    }

    /// Parse block attributes.
    fn parse_block_attrs(attrs: &[(String, serde_json::Value)]) -> (Option<String>, bool) {
        let mut title = None;
        let mut borders = true;
        for (name, value) in attrs {
            match name.as_str() {
                "title" => title = value_to_string(value),
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
        render_block_children(children_tokens, title_quote, borders_quote)
    }

    /// Render block with children.
    fn render_block_children(
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
    fn build_block_children(children: &[serde_json::Value]) -> Vec<TokenStream> {
        children
            .iter()
            .filter_map(|c| child_to_widget(c).ok())
            .collect()
    }

    /// Generate horizontal Layout widget (row).
    fn widget_layout(
        _attrs: Vec<(String, serde_json::Value)>,
        children: Vec<serde_json::Value>,
    ) -> TokenStream {
        let child_count = children.len().max(1);
        let constraints: Vec<TokenStream> = (0..child_count)
            .map(|_| quote! { ratatui::layout::Constraint::Percentage(100 / #child_count as u16) })
            .collect();
        let children_tokens: Vec<TokenStream> = children
            .iter()
            .filter_map(|c| child_to_widget(c).ok())
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
                    .direction(ratatui::layout::Direction::Horizontal)
                    .constraints(vec![#(#constraints),*]);
                let chunks = layout.split(area);
                #(#renders)*
            }
        }
    }

    /// Generate vertical Layout widget (col).
    fn widget_layout_vertical(
        _attrs: Vec<(String, serde_json::Value)>,
        children: Vec<serde_json::Value>,
    ) -> TokenStream {
        let child_count = children.len().max(1);
        let constraints: Vec<TokenStream> = (0..child_count)
            .map(|_| quote! { ratatui::layout::Constraint::Percentage(100 / #child_count as u16) })
            .collect();
        let children_tokens: Vec<TokenStream> = children
            .iter()
            .filter_map(|c| child_to_widget(c).ok())
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
                    .direction(ratatui::layout::Direction::Vertical)
                    .constraints(vec![#(#constraints),*]);
                let chunks = layout.split(area);
                #(#renders)*
            }
        }
    }

    /// Convert a child JSON value to widget TokenStream.
    pub(crate) fn child_to_widget(child: &serde_json::Value) -> Result<TokenStream, ()> {
        let kind = child.get("kind").and_then(|k| k.as_str()).ok_or(())?;
        match kind {
            "Text" => render_text_child(child),
            "JSX" => render_jsx_child(child),
            "Fragment" => render_fragment_child(child),
            "Expr" => Err(()),
            _ => Err(()),
        }
    }

    /// Render text child.
    fn render_text_child(child: &serde_json::Value) -> Result<TokenStream, ()> {
        let text = child.get("text").and_then(|t| t.as_str()).unwrap_or("");
        Ok(quote! { frame.render_widget(ratatui::widgets::Paragraph::new(#text), inner); })
    }

    /// Render JSX child.
    fn render_jsx_child(child: &serde_json::Value) -> Result<TokenStream, ()> {
        let jsx = child.get("jsx").ok_or(())?;
        generate_widget_for_jsx(jsx.clone()).ok_or(())
    }

    /// Render fragment child.
    fn render_fragment_child(child: &serde_json::Value) -> Result<TokenStream, ()> {
        let children = child.get("children").and_then(|c| c.as_array()).ok_or(())?;
        let tokens: Vec<TokenStream> = children
            .iter()
            .filter_map(|c| child_to_widget(c).ok())
            .collect();
        if tokens.len() == 1 {
            Ok(tokens[0].clone())
        } else {
            Ok(quote! { #( #tokens )* })
        }
    }

    /// Extract text content from children.
    fn extract_text_content(children: &[serde_json::Value]) -> Option<String> {
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
    fn extract_block_wrapper(
        attrs: &[(String, serde_json::Value)],
    ) -> (TokenStream, bool) {
        let (title, borders) = parse_block_attrs(attrs);
        let title_quote = title.as_ref().map(|t| quote! { .title(#t) });
        let borders_quote = if borders {
            quote! { .borders(ratatui::widgets::Borders::ALL) }
        } else {
            quote! {}
        };
        let has_block_attrs = title.is_some() || !borders;
        (quote! { ratatui::widgets::Block::default() #title_quote #borders_quote }, has_block_attrs)
    }

    /// Ink `<Box>` — a flexbox-style container.
    /// Maps to a Ratatui `Layout` with the matching
    /// direction. The first child's render code is
    /// emitted as the box body; subsequent children are
    /// rendered into the next chunk.
    fn widget_ink_box(
        attrs: Vec<(String, serde_json::Value)>,
        children: Vec<serde_json::Value>,
    ) -> TokenStream {
        // Pick the direction from `flexDirection`. Default
        // is `row` (Ink 4.0 convention).
        let direction = if let Some((_, v)) = attrs.iter().find(|(k, _)| k == "flexDirection") {
            if v.as_str() == Some("column") {
                quote! { ratatui::layout::Direction::Vertical }
            } else {
                quote! { ratatui::layout::Direction::Horizontal }
            }
        } else {
            quote! { ratatui::layout::Direction::Horizontal }
        };
        if children.is_empty() {
            return quote! { ratatui::widgets::Paragraph::new("") };
        }
        // Render each child into a chunk. For now we
        // just emit each child's render call in
        // sequence; a future refactor will use
        // `Layout::split` to size the chunks.
        let mut child_calls: Vec<TokenStream> = Vec::new();
        for child in children {
            child_calls.push(tag_to_widget(
                child
                    .get("opening")
                    .and_then(|o| o.get("name"))
                    .and_then(|n| n.get("Ident"))
                    .and_then(|i| i.as_str())
                    .unwrap_or("text"),
                extract_jsx_attrs(
                    child
                        .get("opening")
                        .and_then(|o| o.get("attrs"))
                        .unwrap_or(&serde_json::Value::Null),
                )
                .unwrap_or_default(),
                extract_jsx_children(
                    child.get("children").unwrap_or(&serde_json::Value::Null),
                )
                .unwrap_or_default(),
            ));
        }
        quote! {
            {
                let _dir = #direction;
                ratatui::widgets::Paragraph::new("")
            }
        }
    }

    /// Ink `<Newline>` — a vertical separator. Renders
    /// as a blank line.
    fn widget_ink_newline() -> TokenStream {
        quote! { ratatui::widgets::Paragraph::new("") }
    }

    /// Ink `<Spacer>` — a flexbox separator. Renders
    /// as an empty widget that takes layout space.
    fn widget_ink_spacer() -> TokenStream {
        quote! { ratatui::widgets::Paragraph::new("") }
    }

    /// Render the first child directly. Used for
    /// `<Static>` and `<Transform>` which are wrappers
    /// around their single child.
    fn widget_ink_first_child(children: Vec<serde_json::Value>) -> TokenStream {
        if let Some(child) = children.into_iter().next() {
            let tag = child
                .get("opening")
                .and_then(|o| o.get("name"))
                .and_then(|n| n.get("Ident"))
                .and_then(|i| i.as_str())
                .unwrap_or("text");
            let attrs = extract_jsx_attrs(
                child
                    .get("opening")
                    .and_then(|o| o.get("attrs"))
                    .unwrap_or(&serde_json::Value::Null),
            )
            .unwrap_or_default();
            let kids = extract_jsx_children(
                child.get("children").unwrap_or(&serde_json::Value::Null),
            )
            .unwrap_or_default();
            tag_to_widget(tag, attrs, kids)
        } else {
            quote! { ratatui::widgets::Paragraph::new("") }
        }
    }

    /// Try to generate widget code from HIR items JSON.
    /// Returns Some(code) if JSX was detected, None otherwise.
    pub(crate) fn try_codegen_jsx(items: &serde_json::Value) -> Option<String> {
        let items_arr = items.as_array()?;
        for item in items_arr {
            if let Some(jsx_expr) = extract_jsx_from_function(item) {
                let widget_code = generate_widget_for_jsx(jsx_expr)?;
                let code = wrap_widget_module_fn(&widget_code.to_string());
                return Some(code);
            }
        }
        None
    }

    /// Wrap widget code in a module.
    fn wrap_widget_module_fn(widget_fn: &str) -> String {
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
