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
    /// The parser emits the body as a flat array of
    /// statements (`[stmt0, stmt1, ...]`). The
    /// previous version expected `body.Block.stmts`,
    /// which is the shape of a manually-constructed
    /// fixture, not the parser output.
    pub(crate) fn find_jsx_in_body(body: &serde_json::Value) -> Option<serde_json::Value> {
        if let Some(stmts) = body.as_array() {
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
    /// The parser emits the return value as
    /// `{arg: {JSX: {opening, children, ...}}}`.
    /// We unwrap the single-key `{JSX: ...}` to
    /// reach the actual JSX object.
    fn find_jsx_in_return(stmt: &serde_json::Value) -> Option<serde_json::Value> {
        let arg = stmt.get("arg")?;
        let unwrapped = unwrap_jsx(arg);
        if is_jsx_expr(&unwrapped) {
            return Some(unwrapped);
        }
        find_jsx_in_expr(&unwrapped)
    }

    /// Find JSX in expression statement.
    /// Same shape as Return: the expression is
    /// wrapped in `{JSX: ...}` by the parser.
    fn find_jsx_in_expr_stmt(stmt: &serde_json::Value) -> Option<serde_json::Value> {
        let expr = stmt.get("expr")?;
        let unwrapped = unwrap_jsx(expr);
        if is_jsx_expr(&unwrapped) {
            return Some(unwrapped);
        }
        find_jsx_in_expr(&unwrapped)
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
    /// Accepts both shapes:
    ///   - `{opening: {...}, children: [...]}`
    ///     (fixture / direct JSX)
    ///   - `{"JSX": {opening, children, ...}}`
    ///     (parser single-key wrapper)
    fn is_jsx_expr(val: &serde_json::Value) -> bool {
        if val.get("opening").is_some() && val.get("children").is_some() {
            return true;
        }
        if let Some(inner) = val.get("JSX") {
            return inner.get("opening").is_some() && inner.get("children").is_some();
        }
        false
    }

    /// Extract the JSX expression object out of a
    /// single-key `{JSX: ...}` wrapper if present.
    fn unwrap_jsx(val: &serde_json::Value) -> serde_json::Value {
        if val.get("JSX").is_some() && val.get("opening").is_none() {
            val.get("JSX").cloned().unwrap_or_else(|| val.clone())
        } else {
            val.clone()
        }
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
    /// The parser emits children as single-key
    /// objects: `{Text: "..."}`, `{JSX: {...}}`,
    /// `{Fragment: {children: [...]}}`, or
    /// `{kind: "Expr", expr: ...}`. Check the
    /// single-key shape first (that's the common
    /// case), then fall back to the `kind`
    /// discriminator for expression children.
    fn jsx_child_to_value(
        child: &serde_json::Value,
    ) -> Option<Option<serde_json::Value>> {
        if child.as_str().is_some() {
            return jsx_string_child(child.as_str().unwrap());
        }
        // Single-key shapes first (most common).
        if let Some(text) = child.get("Text").and_then(|t| t.as_str()) {
            return jsx_text_value(text);
        }
        if child.get("JSX").is_some() || child.get("jsx").is_some() {
            return jsx_jsx_value(child);
        }
        if child.get("Fragment").is_some() {
            return jsx_fragment_value(child);
        }
        // Then the `kind` discriminator for
        // expression children. `Text` here means
        // the fixture shape `{kind: "Text", text:
        // "..."}`. `Expr` and `Spread` are runtime
        // shapes.
        if let Some(kind) = child.get("kind").and_then(|k| k.as_str()) {
            return match kind {
                "Text" => {
                    let text = child
                        .get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    Some(Some(serde_json::json!({
                        "kind": "Text",
                        "text": text,
                    })))
                }
                "Expr" => Some(Some(child.clone())),
                "Spread" => Some(None),
                _ => Some(None),
            };
        }
        Some(None)
    }

    /// Wrap a raw string child in the codegen's
    /// `{"kind": "Text", "text": "..."}` shape.
    /// This is the only Text case left here; the
    /// single-key `{"Text": "..."}` case is handled
    /// by `jsx_text_value` below.
    fn jsx_string_child(text: &str) -> Option<Option<serde_json::Value>> {
        Some(Some(serde_json::json!({
            "kind": "Text",
            "text": text,
        })))
    }

    /// Wrap a text string in the codegen's
    /// `{"kind": "Text", "text": "..."}` shape.
    fn jsx_text_value(text: &str) -> Option<Option<serde_json::Value>> {
        Some(Some(serde_json::json!({
            "kind": "Text",
            "text": text,
        })))
    }

    /// Pull the JSX object out of `{JSX: ...}`.
    /// The parser emits single-key `{JSX: ...}`
    /// and we sometimes see the normalized
    /// `{jsx: ...}` from earlier passes — accept
    /// both.
    fn jsx_jsx_value(child: &serde_json::Value) -> Option<Option<serde_json::Value>> {
        let jsx_expr = child
            .get("JSX")
            .or_else(|| child.get("jsx"))?
            .clone();
        Some(Some(serde_json::json!({
            "kind": "JSX",
            "jsx": jsx_expr,
        })))
    }

    /// Fragment: recurse on its children.
    fn jsx_fragment_value(child: &serde_json::Value) -> Option<Option<serde_json::Value>> {
        let frag_children = child.get("Fragment")?.get("children")?;
        let children = extract_jsx_children(frag_children)?;
        Some(Some(serde_json::json!({
            "kind": "Fragment",
            "children": children,
        })))
    }
    pub(crate) fn value_to_string(val: &serde_json::Value) -> Option<String> {
        match val {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Object(obj) => parse_expr_value(obj),
            _ => None,
        }
    }

    /// Parse a value object into a Rust-source
    /// string. Handles the HIR's single-key shapes:
    ///   `{"String": "foo"}` → `"foo"`
    ///   `{"Number": 1.5}`  → `"1.5"`
    ///   `{"Ident": {"name": "x"}}` → `"x"`
    ///   `{"Bool": true}` → `"true"`
    ///   `{kind: "Expr", expr: {...}}` → recurses.
    pub(crate) fn parse_expr_value(
        obj: &serde_json::Map<String, serde_json::Value>,
    ) -> Option<String> {
        // Expression-child wrapper.
        if let Some(expr) = obj.get("Expr") {
            return parse_expr_inner(expr);
        }
        parse_expr_inner_map(obj)
    }

    /// Inner parser for a single expression object.
    fn parse_expr_inner(obj: &serde_json::Value) -> Option<String> {
        parse_expr_inner_value(obj)
    }

    /// Same as `parse_expr_inner` but takes a Value
    /// (auto-extracts the map). Used when the caller
    /// has a `Value` not a `Map`.
    fn parse_expr_inner_value(obj: &serde_json::Value) -> Option<String> {
        let map = obj.as_object()?;
        parse_expr_inner_map(map)
    }

    /// Core parser, takes the map directly.
    fn parse_expr_inner_map(
        map: &serde_json::Map<String, serde_json::Value>,
    ) -> Option<String> {
        let map = map;
        // Tuple-struct-like `{"kind": "X", "0": value}`.
        if let (Some(kind), Some(val)) = (map.get("kind"), map.get("0")) {
            return match kind.as_str()? {
                "String" => val.as_str().map(String::from),
                "Number" => val.as_f64().map(|n| n.to_string()),
                "Bool" => val.as_bool().map(|b| b.to_string()),
                "Ident" => val
                    .as_object()
                    .and_then(|o| o.get("name"))
                    .and_then(|n| n.as_str())
                    .map(String::from),
                _ => None,
            };
        }
        // Single-key wrappers.
        if let Some(s) = map.get("String").and_then(|v| v.as_str()) {
            return Some(s.to_string());
        }
        if let Some(n) = map.get("Number").and_then(|v| v.as_f64()) {
            return Some(n.to_string());
        }
        if let Some(b) = map.get("Bool").and_then(|v| v.as_bool()) {
            return Some(b.to_string());
        }
        if let Some(name) = map
            .get("Ident")
            .and_then(|v| v.as_object())
            .and_then(|o| o.get("name"))
            .and_then(|n| n.as_str())
        {
            return Some(name.to_string());
        }
        None
    }

    // ============================================================================
    // Widget generation (TokenStream building)
    // ============================================================================

    /// Extract JSX from a HIR declaration item.
    /// The real HIR uses single-key objects: items
    /// are `{Import: ...}`, `{Decl: {Function: ...}}`,
    /// `{Stmt: ...}`. The previous version looked for
    /// `"type": "Decl"` + `"kind": "Function"`, which
    /// was the shape of a hand-rolled test fixture,
    /// not the parser output.
    pub(crate) fn extract_jsx_from_function(item: &serde_json::Value) -> Option<serde_json::Value> {
        let decl = item.get("Decl")?;
        let func = decl.get("Function")?;
        let body = func.get("body")?;
        find_jsx_in_body(body)
    }

    /// Generate widget code from JSX expression.
    /// Uses the Ink codegen path (real
    /// `runts_ink::*` builder calls), not the
    /// Ratatui-stub path.
    pub(crate) fn generate_widget_for_jsx(jsx: serde_json::Value) -> Option<TokenStream> {
        let opening = jsx.get("opening")?;
        let name = jsx_name_to_string(opening.get("name")?)?;
        let attrs = extract_jsx_attrs(opening.get("attrs")?)?;
        let children = extract_jsx_children(jsx.get("children")?)?;
        Some(tag_to_ink(&name, attrs, children))
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
        Ok(quote! { runts_ink::Text::new(#text) })
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
    /// The HIR emits text children as either the
    /// fixture shape `{kind: "Text", text: "..."}`
    /// or the parser shape `{"Text": "..."}`. We
    /// accept both.
    fn extract_text_content(children: &[serde_json::Value]) -> Option<String> {
        let mut text = String::new();
        for child in children {
            // Fixture shape (test data).
            if let Some(s) = child
                .get("text")
                .and_then(|v| v.as_str())
                .filter(|_| child.get("kind").and_then(|k| k.as_str()) == Some("Text"))
            {
                text.push_str(s);
                continue;
            }
            // Parser shape: single-key `{"Text": "..."}`.
            if let Some(s) = child.get("Text").and_then(|v| v.as_str()) {
                text.push_str(s);
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
    /// Emits a `runts_ink::Box` builder chain that
    /// returns a `runts_ink::VNode` (via `.into()`).
    /// The runtime uses Taffy for layout and Ratatui
    /// for drawing, so this is the same renderer Ink
    /// uses under the hood.
    fn widget_ink_box(
        attrs: Vec<(String, serde_json::Value)>,
        children: Vec<serde_json::Value>,
    ) -> TokenStream {
        // Pick the starting builder from
        // `flexDirection` so that `row` (Ink's
        // default) doesn't trigger the codegen
        // warning for an unused `flex_direction`
        // call. We still emit `.flex_direction(...)`
        // if the prop is explicit so user intent is
        // preserved.
        let has_direction = attrs
            .iter()
            .any(|(k, _)| k == "flexDirection");
        let mut builder: TokenStream = if let Some((_, v)) =
            attrs.iter().find(|(k, _)| k == "flexDirection")
        {
            let dir_tok = flex_direction_token(v);
            quote! { runts_ink::Box::new().flex_direction(#dir_tok) }
        } else {
            quote! { runts_ink::Box::new() }
        };
        let _ = has_direction;
        // Apply remaining Box props.
        for (name, value) in &attrs {
            if name == "flexDirection" {
                continue;
            }
            if let Some(call) = box_prop_call(name, value) {
                builder = quote! { #builder #call };
            }
        }
        // Append each child as a `.child(expr)` call.
        for child in &children {
            let child_expr = child_to_vnode(child);
            builder = quote! { #builder.child(#child_expr) };
        }
        // Return the Box, NOT `.into()`. The
        // parent caller wraps `.child(impl
        // Into<VNode>)` so a bare Box auto-
        // converts. Adding `.into()` here makes
        // nested Box children fail E0283.
        builder
    }

    /// Ink `<block title="...">` — a bordered Box
    /// with a title. The title becomes the first
    /// Text child of the Box; the other children
    /// follow. The Box gets a default
    /// `border_style(classic)` so the title and
    /// children have a visible border.
    fn widget_ink_block(
        attrs: Vec<(String, serde_json::Value)>,
        children: Vec<serde_json::Value>,
    ) -> TokenStream {
        // Convert the legacy `<block>` attrs
        // (`title`, `borders`) to Box-style attrs
        // (`borderStyle`, default `classic`).
        let mut box_attrs: Vec<(String, serde_json::Value)> = Vec::new();
        let mut title: Option<String> = None;
        let mut has_border_attr = false;
        for (k, v) in &attrs {
            match k.as_str() {
                "title" => {
                    title = v.as_str().map(|s| s.to_string());
                }
                "borders" => {
                    if v.as_bool() == Some(false) {
                        // No border — skip borderStyle.
                    } else {
                        has_border_attr = true;
                    }
                }
                _ => {
                    box_attrs.push((k.clone(), v.clone()));
                }
            }
        }
        if !has_border_attr {
            box_attrs.push((
                "borderStyle".to_string(),
                serde_json::Value::String("classic".to_string()),
            ));
        }
        // Prepend a Text child for the title so it
        // appears as the top row of the bordered box.
        let mut final_children: Vec<serde_json::Value> = Vec::new();
        if let Some(t) = title {
            final_children.push(serde_json::json!({
                "kind": "Text",
                "text": t,
            }));
        }
        final_children.extend(children);
        widget_ink_box(box_attrs, final_children)
    }

    /// Ink `<Text>` — a styled string.
    /// Emits a `runts_ink::Text` builder chain that
    /// returns a `runts_ink::VNode`.
    fn widget_ink_text_call(
        attrs: Vec<(String, serde_json::Value)>,
        content: String,
    ) -> TokenStream {
        let mut builder = quote! { runts_ink::Text::new(#content) };
        for (name, value) in &attrs {
            match name.as_str() {
                "bold" => {
                    if truthy(value) {
                        builder = quote! { #builder.bold() };
                    }
                }
                "italic" => {
                    if truthy(value) {
                        builder = quote! { #builder.italic() };
                    }
                }
                "underline" => {
                    if truthy(value) {
                        builder = quote! { #builder.underline() };
                    }
                }
                "strikethrough" => {
                    if truthy(value) {
                        builder = quote! { #builder.strikethrough() };
                    }
                }
                "dimColor" | "dimcolor" => {
                    if truthy(value) {
                        builder = quote! { #builder.dim() };
                    }
                }
                "inverse" => {
                    if truthy(value) {
                        builder = quote! { #builder.inverse() };
                    }
                }
                "color" => {
                    if let Some(color_tok) = color_token(value) {
                        builder = quote! { #builder.color(#color_tok) };
                    }
                }
                "backgroundColor" | "backgroundcolor" => {
                    if let Some(color_tok) = color_token(value) {
                        builder = quote! { #builder.background_color(#color_tok) };
                    }
                }
                "wrap" => {
                    let wrap_tok = wrap_mode_token(value);
                    builder = quote! { #builder.wrap(#wrap_tok) };
                }
                _ => {
                    // Unknown prop — silently ignore so
                    // the v0.1 codegen stays forward-
                    // compatible with new props.
                }
            }
        }
        // No trailing `.into()`: `Box::child` takes
        // `impl Into<VNode>`, so the bare `Text`
        // expression chains cleanly through
        // `.child().child().into()` without the
        // type-inference ambiguity that `.into()`
        // on intermediate values causes.
        quote! { #builder }
    }

    /// Ink `<Newline>` — a vertical separator. Emits
    /// `runts_ink::Newline::new().into()`.
    fn widget_ink_newline() -> TokenStream {
        quote! { runts_ink::Newline::new().into() }
    }

    /// Ink `<Spacer>` — a flexbox separator. Emits
    /// `runts_ink::Spacer::new().into()`.
    fn widget_ink_spacer() -> TokenStream {
        quote! { runts_ink::Spacer::new().into() }
    }

    /// Convert a JSX child (already a normalized
    /// JSX object — opening, attrs, children) to a
    /// `TokenStream` expression that produces a
    /// `runts_ink::VNode`.
    fn child_to_vnode(child: &serde_json::Value) -> TokenStream {
        // Short-circuit: a normalized `{kind: "Text",
        // text: "..."}` child (from
        // `extract_jsx_children`) is a bare Text,
        // not a JSX element. Build it directly.
        if let Some(kind) = child.get("kind").and_then(|k| k.as_str()) {
            // Distinguish: a bare Text wrapper
            // (`{kind: "Text", text: "..."}`) from a
            // real JSX element that also happens to
            // have a `kind` field (`{kind: "JSX",
            // opening: {...}, children: [...]}`).
            match kind {
                "Text"
                    if child.get("text").is_some()
                        && child.get("opening").is_none() =>
                {
                    let text = child.get("text").and_then(|v| v.as_str()).unwrap_or("");
                    // No `.into()` here: `Box::child`
                    // takes `impl Into<VNode>`, and
                    // emitting the bare expression lets
                    // Rust infer the type (otherwise
                    // chained `.child().child()` fails
                    // with E0283).
                    return quote! { runts_ink::Text::new(#text) };
                }
                "JSX" if child.get("jsx").is_some() && child.get("opening").is_none() => {
                    // `{kind: "JSX", jsx: {...}}` —
                    // wrapped JSX, recurse.
                    if let Some(inner) = child.get("jsx") {
                        return child_to_vnode(inner);
                    }
                }
                _ => {}
            }
        }
        let opening = child.get("opening");
        let tag = opening
            .and_then(|o| o.get("name"))
            .and_then(|n| n.get("Ident"))
            .and_then(|i| i.as_str())
            .unwrap_or("text");
        let attrs = opening
            .and_then(|o| o.get("attrs"))
            .map(|a| extract_jsx_attrs(a).unwrap_or_default())
            .unwrap_or_default();
        let kids = extract_jsx_children(
            child.get("children").unwrap_or(&serde_json::Value::Null),
        )
        .unwrap_or_default();
        tag_to_ink(tag, attrs, kids)
    }

    /// Map an Ink tag to a `runts_ink::VNode` expr.
    /// This is the Ink-specific dispatcher; the
    /// legacy `tag_to_widget` still exists for the
    /// Ratatui-stub path. The legacy tags
    /// (`text`, `block`, `row`, `col`,
    /// `paragraph`) are intentionally NOT mapped
    /// here — they go through `tag_to_widget` so
    /// the legacy examples still build.
    fn tag_to_ink(
        tag: &str,
        attrs: Vec<(String, serde_json::Value)>,
        children: Vec<serde_json::Value>,
    ) -> TokenStream {
        match tag {
            "Box" | "box" => widget_ink_box(attrs, children),
            "block" => {
                // `<block title="...">` lowers to a
                // bordered Box with the title as the
                // first Text child.
                widget_ink_block(attrs, children)
            }
            "paragraph" | "Text" | "text" | "inktext" => {
                // `<paragraph>foo</paragraph>`,
                // `<Text>foo</Text>`, and
                // `<text>foo</text>` are all just a
                // Text.
                let text = collect_text_from_children(&children);
                widget_ink_text_call(attrs, text)
            }
            "row" | "col" => {
                // `<row>` / `<col>` are flex-direction
                // hints on a Box.
                let mut box_attrs = attrs;
                let dir = if tag == "row" {
                    "row"
                } else {
                    "column"
                };
                box_attrs.push((
                    "flexDirection".to_string(),
                    serde_json::Value::String(dir.to_string()),
                ));
                widget_ink_box(box_attrs, children)
            }
            "Newline" => widget_ink_newline(),
            "Spacer" => widget_ink_spacer(),
            "Static" | "Transform" => widget_ink_first_child(children),
            _ => {
                // Unknown intrinsic — fall back to a
                // Text with the tag name. This matches
                // the previous placeholder behaviour.
                let label = tag.to_string();
                widget_ink_text_call(Vec::new(), label)
            }
        }
    }

    /// Flatten JSX children to a single text string
    /// (whitespace-separated). Used by `<Text>` to
    /// extract its content. For the bordered
    /// example, all `<Text>` children are either
    /// text strings or `{''}` empty expressions,
    /// so this produces the right concatenation.
    fn collect_text_from_children(children: &[serde_json::Value]) -> String {
        let mut out = String::new();
        for raw in children {
            // Children at this layer are the
            // normalized shape (see
            // `extract_jsx_children`): each is
            // `{kind: "Text", text: "..."}` or
            // `{kind: "JSX", jsx: {...}}`.
            if let Some(text) = raw.get("text").and_then(|t| t.as_str()) {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(text);
            } else if let Some(jsx) = raw.get("jsx") {
                // Recurse into a nested JSX element
                // to get its text content.
                let nested = extract_jsx_children(
                    jsx.get("children").unwrap_or(&serde_json::Value::Null),
                )
                .unwrap_or_default();
                let inner = collect_text_from_children(&nested);
                if !inner.is_empty() {
                    if !out.is_empty() {
                        out.push(' ');
                    }
                    out.push_str(&inner);
                }
            }
            // Expression children and Fragments are
            // skipped — `<Text>{count}</Text>` would
            // need an interpolation pipeline that
            // we don't have yet (useState-equivalent
            // comes with the rquickjs dev path).
        }
        out
    }

    /// Map a `flexDirection` JSON value to a
    /// `runts_ink::FlexDirection` token. Returns
    /// Map a string to a FlexDirection variant.
    /// Accepts both raw strings (fixture) and
    /// `{String: "..."}` / `{Expr: {String: "..."}}`
    /// (parser).
    fn flex_direction_token(value: &serde_json::Value) -> TokenStream {
        // Try as plain string first.
        if let Some(s) = value.as_str() {
            return flex_dir_for_str(s);
        }
        // Parser shape: single-key wrappers.
        if let Some(s) = value.get("String").and_then(|v| v.as_str()) {
            return flex_dir_for_str(s);
        }
        if let Some(expr) = value.get("Expr") {
            if let Some(s) = expr.get("String").and_then(|v| v.as_str()) {
                return flex_dir_for_str(s);
            }
        }
        flex_dir_for_str("")
    }

    /// Map a flex-direction string to a token.
    fn flex_dir_for_str(s: &str) -> TokenStream {
        match s {
            "column" => quote! { runts_ink::FlexDirection::Column },
            "row-reverse" | "rowReverse" => {
                quote! { runts_ink::FlexDirection::RowReverse }
            }
            "column-reverse" | "columnReverse" => {
                quote! { runts_ink::FlexDirection::ColumnReverse }
            }
            _ => quote! { runts_ink::FlexDirection::Row },
        }
    }

    /// Map a `borderStyle` JSON value to a
    /// `runts_ink::BorderStyle` token.
    fn border_style_token(value: &serde_json::Value) -> TokenStream {
        if let Some(s) = value.as_str() {
            return match s {
                "round" => quote! { runts_ink::BorderStyle::Round },
                "double" => quote! { runts_ink::BorderStyle::Double },
                "bold" => quote! { runts_ink::BorderStyle::Bold },
                "classic" => quote! { runts_ink::BorderStyle::Classic },
                _ => quote! { runts_ink::BorderStyle::Single },
            };
        }
        quote! { runts_ink::BorderStyle::Single }
    }

    /// Map a `color` JSON value to a
    /// `runts_ink::Color` token.
    fn color_token(value: &serde_json::Value) -> Option<TokenStream> {
        let name = value.as_str()?;
        let tok = match name.to_ascii_lowercase().as_str() {
            "default" => quote! { runts_ink::Color::Default },
            "black" => quote! { runts_ink::Color::Black },
            "red" => quote! { runts_ink::Color::Red },
            "green" => quote! { runts_ink::Color::Green },
            "yellow" => quote! { runts_ink::Color::Yellow },
            "blue" => quote! { runts_ink::Color::Blue },
            "magenta" => quote! { runts_ink::Color::Magenta },
            "cyan" => quote! { runts_ink::Color::Cyan },
            "white" => quote! { runts_ink::Color::White },
            "gray" | "grey" => quote! { runts_ink::Color::Gray },
            "blackbright" => quote! { runts_ink::Color::BrightBlack },
            "redbright" => quote! { runts_ink::Color::BrightRed },
            "greenbright" => quote! { runts_ink::Color::BrightGreen },
            "yellowbright" => quote! { runts_ink::Color::BrightYellow },
            "bluebright" => quote! { runts_ink::Color::BrightBlue },
            "magentabright" => quote! { runts_ink::Color::BrightMagenta },
            "cyanbright" => quote! { runts_ink::Color::BrightCyan },
            "whitebright" => quote! { runts_ink::Color::BrightWhite },
            _ => return None,
        };
        Some(tok)
    }

    /// Map a `wrap` JSON value to a
    /// `runts_ink::Wrap` token.
    fn wrap_mode_token(value: &serde_json::Value) -> TokenStream {
        if let Some(s) = value.as_str() {
            return match s {
                "wrap" => quote! { runts_ink::Wrap::Wrap },
                "truncate-end" | "truncateEnd" | "end" => {
                    quote! { runts_ink::Wrap::TruncateEnd }
                }
                "truncate-middle" | "truncateMiddle" | "middle" => {
                    quote! { runts_ink::Wrap::TruncateMiddle }
                }
                _ => quote! { runts_ink::Wrap::NoWrap },
            };
        }
        quote! { runts_ink::Wrap::NoWrap }
    }

    /// Convert a non-string JSON value to a string
    /// literal. Used for numeric prop values like
    /// `paddingX={2}`.
    fn json_value_to_string(value: &serde_json::Value) -> Option<String> {
        match value {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            serde_json::Value::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }

    /// Is this JSON value truthy? Booleans follow
    /// JS semantics; everything else is treated as
    /// truthy so a missing attr value still
    /// enables the flag.
    fn truthy(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Bool(b) => *b,
            serde_json::Value::Null => false,
            _ => true,
        }
    }

    /// Map a Box prop name + value to a builder
    /// call. Returns None for unrecognised props.
    fn box_prop_call(
        name: &str,
        value: &serde_json::Value,
    ) -> Option<TokenStream> {
        match name {
            "padding" => {
                let n = value.as_u64()?;
                Some(quote! { .padding(#n) })
            }
            "paddingX" | "paddingx" => {
                let n = value.as_u64()?;
                Some(quote! { .padding_x(#n) })
            }
            "paddingY" | "paddingy" => {
                let n = value.as_u64()?;
                Some(quote! { .padding_y(#n) })
            }
            "paddingTop" | "paddingtop" => {
                let n = value.as_u64()?;
                Some(quote! { .padding_top(#n) })
            }
            "paddingBottom" | "paddingbottom" => {
                let n = value.as_u64()?;
                Some(quote! { .padding_bottom(#n) })
            }
            "paddingLeft" | "paddingleft" => {
                let n = value.as_u64()?;
                Some(quote! { .padding_left(#n) })
            }
            "paddingRight" | "paddingright" => {
                let n = value.as_u64()?;
                Some(quote! { .padding_right(#n) })
            }
            "margin" => {
                let n = value.as_u64()?;
                Some(quote! { .margin(#n) })
            }
            "width" | "height" => {
                let n = value.as_u64()?;
                let m = quote::format_ident!("{}", name);
                Some(quote! { .#m(#n) })
            }
            "flexGrow" | "flexgrow" => {
                let n = value.as_f64()?;
                Some(quote! { .flex_grow(#n) })
            }
            "flexShrink" | "flexshrink" => {
                let n = value.as_f64()?;
                Some(quote! { .flex_shrink(#n) })
            }
            "gap" => {
                let n = value.as_u64()?;
                Some(quote! { .gap(#n) })
            }
            "rowGap" | "rowgap" => {
                let n = value.as_u64()?;
                Some(quote! { .row_gap(#n) })
            }
            "columnGap" | "columngap" => {
                let n = value.as_u64()?;
                Some(quote! { .column_gap(#n) })
            }
            "alignItems" | "alignitems" => {
                let s = value.as_str()?;
                let tok = match s {
                    "flex-start" | "flexStart" | "start" => {
                        quote! { runts_ink::AlignItems::FlexStart }
                    }
                    "center" => quote! { runts_ink::AlignItems::Center },
                    "flex-end" | "flexEnd" | "end" => {
                        quote! { runts_ink::AlignItems::FlexEnd }
                    }
                    "stretch" => quote! { runts_ink::AlignItems::Stretch },
                    _ => return None,
                };
                Some(quote! { .align_items(#tok) })
            }
            "justifyContent" | "justifycontent" => {
                let s = value.as_str()?;
                let tok = match s {
                    "flex-start" | "flexStart" | "start" => {
                        quote! { runts_ink::JustifyContent::FlexStart }
                    }
                    "center" => quote! { runts_ink::JustifyContent::Center },
                    "flex-end" | "flexEnd" | "end" => {
                        quote! { runts_ink::JustifyContent::FlexEnd }
                    }
                    "space-between" | "spaceBetween" => {
                        quote! { runts_ink::JustifyContent::SpaceBetween }
                    }
                    "space-around" | "spaceAround" => {
                        quote! { runts_ink::JustifyContent::SpaceAround }
                    }
                    _ => return None,
                };
                Some(quote! { .justify_content(#tok) })
            }
            "borderStyle" | "borderstyle" => {
                let tok = border_style_token(value);
                Some(quote! { .border_style(#tok) })
            }
            "borderColor" | "bordercolor" => {
                let color = color_token(value)?;
                Some(quote! { .border_color(#color) })
            }
            "backgroundColor" | "backgroundcolor" => {
                let color = color_token(value)?;
                Some(quote! { .background_color(#color) })
            }
            _ => {
                let _ = json_value_to_string(value);
                None
            }
        }
    }
    /// Render the first child directly. Used for
    /// `<Static>` and `<Transform>` which are wrappers
    /// around their single child.
    fn widget_ink_first_child(children: Vec<serde_json::Value>) -> TokenStream {
        if let Some(child) = children.into_iter().next() {
            child_to_vnode(&child)
        } else {
            // Empty `<Static>` / `<Transform>` becomes
            // an empty Text — the renderer just draws
            // a blank line, which matches Ink.
            quote! { runts_ink::Text::new(String::new()).into() }
        }
    }

    /// Try to generate widget code from HIR items JSON.
    /// Returns Some(code) if JSX was detected, None otherwise.
    /// The returned code is a complete Rust file
    /// with `fn main()` that uses
    /// `runts_ink::render_to_string` to produce the
    /// first-frame output of the JSX tree, then
    /// writes the result to stdout.
    pub(crate) fn try_codegen_jsx(items: &serde_json::Value) -> Option<String> {
        let items_arr = items.as_array()?;
        for item in items_arr {
            if let Some(jsx_expr) = extract_jsx_from_function(item) {
                let widget_code = generate_widget_for_jsx(jsx_expr)?;
                let code = wrap_ink_main(&widget_code.to_string());
                return Some(code);
            }
        }
        None
    }

    /// Wrap a VNode expression in a `fn main()` that
    /// builds the tree, calls `render_to_string` to
    /// get the first-frame grid, and writes it to
    /// stdout. This matches actual Ink's behaviour
    /// for a non-interactive app: write the rendered
    /// grid to the terminal.
    fn wrap_ink_main(vnode_expr: &str) -> String {
        format!(
            r#"//! Ink app entry: generated by runts-ratatui 0.1
//
// The VNode expression below was codegen'd from the
// JSX in the user's .tsx file. The same expression
// would be the root of a React tree in actual Ink.

use runts_ink;

fn main() -> anyhow::Result<()> {{
    // The codegen'd vnode_expr is a builder
    // chain (e.g. `runts_ink::Box::new()...`)
    // which produces a Box or Text value.
    // We `.into()` it to convert to VNode.
    let root: runts_ink::VNode = ({vnode_expr}).into();
    let rendered = runts_ink::render_to_string(
        root,
        runts_ink::RenderOptions::default(),
    )?;
    print!("{{}}", rendered);
    Ok(())
}}
"#
        )
    }
}
