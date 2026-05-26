//! html! macro - JSX-like syntax for building VNodes
//!
//! Transforms JSX-like syntax into Rust VNode construction:
//!
//! ```ignore
//! html! { <div class="foo">Hello {name}</div> }
//! ```
//!
//! Becomes:
//!
//! ```rust
//! VNode::Element {
//!     tag: "div".into(),
//!     attrs: vec![("class", AttrValue::String("foo".into()))].into_iter().collect(),
//!     children: vec![
//!         VNode::Text { value: "Hello ".into() },
//!         VNode::Text { value: name.to_string() },
//!     ],
//!     events: HashMap::new(),
//! }
//! ```

use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::parse::{Parse, ParseStream, Result as SynResult};
use syn::{block, Expr, Ident, Token};

// ============================================================================
// Token Types
// ============================================================================

enum HtmlNode {
    Element(HtmlElement),
    Text(String),
    Expr(Expr),
    Fragment(Vec<HtmlNode>),
}

struct HtmlElement {
    name: HtmlTagName,
    attrs: Vec<HtmlAttr>,
    children: Vec<HtmlNode>,
    self_closing: bool,
}

enum HtmlTagName {
    BuiltIn(String),
    Component(Expr),
}

enum HtmlAttr {
    Normal { name: String, value: HtmlAttrValue },
    Event { name: String, handler: Expr },
    Spread { expr: Expr },
    Bool { name: String },
}

enum HtmlAttrValue {
    Str(String),
    Expr(Expr),
}

// ============================================================================
// Parser
// ============================================================================

struct HtmlParser {
    tokens: Vec<proc_macro2::Token>,
    pos: usize,
}

impl HtmlParser {
    fn new(tokens: Vec<proc_macro2::Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&proc_macro2::Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<proc_macro2::Token> {
        let token = self.tokens.get(self.pos).cloned();
        self.pos += 1;
        token
    }

    fn parse_html(&mut self) -> Result<HtmlNode, String> {
        match self.peek() {
            Some(token) if is_punct(token, '<') => {
                self.parse_element()
            }
            Some(token) if is_ident(token, "Fragment") && self.peek_n(1).map(|t| is_punct(t, '{')).unwrap_or(false) => {
                self.parse_fragment()
            }
            _ => Err("Expected <tag> or Fragment".to_string()),
        }
    }

    fn parse_element(&mut self) -> Result<HtmlNode, String> {
        // Skip '<'
        self.advance();

        let name = self.parse_tag_name()?;

        // Parse attributes
        let mut attrs = Vec::new();
        let mut self_closing = false;

        while let Some(token) = self.peek() {
            if is_punct(token, '/') && self.peek_n(1).map(|t| is_punct(t, '>')).unwrap_or(false) {
                self.advance(); // /
                self.advance(); // >
                self_closing = true;
                break;
            }
            if is_punct(token, '>') {
                self.advance();
                break;
            }
            attrs.push(self.parse_attr()?);
        }

        // Parse children
        let mut children = Vec::new();
        if !self_closing {
            loop {
                match self.peek() {
                    None => break Err("Unclosed tag".to_string()),
                    Some(token) if is_punct(token, '<') => {
                        if self.peek_n(1).map(|t| is_punct(t, '/')).unwrap_or(false) {
                            // Closing tag
                            self.advance(); // <
                            self.advance(); // /
                            self.skip_closing_tag()?;
                            break;
                        }
                        children.push(self.parse_element()?);
                    }
                    Some(token) if is_punct(token, '{') => {
                        self.advance(); // {
                        children.push(self.parse_expr_or_spread()?);
                        // Expect }
                        if !matches!(self.peek(), Some(t) if is_punct(t, '}')) {
                            return Err("Expected '}'".to_string());
                        }
                        self.advance();
                    }
                    Some(token) if is_punct(token, '}') => {
                        // Empty expression
                        break;
                    }
                    _ => {
                        // Text content
                        if let Some(text) = self.parse_text_content() {
                            if !text.trim().is_empty() {
                                children.push(HtmlNode::Text(text));
                            }
                        }
                    }
                }
            }
        }

        Ok(HtmlNode::Element(HtmlElement {
            name,
            attrs,
            children,
            self_closing,
        }))
    }

    fn parse_tag_name(&mut self) -> Result<HtmlTagName, String> {
        match self.advance() {
            Some(token) => {
                let ident = extract_ident(&token).ok_or("Expected identifier")?;
                
                // Check if it's a component (starts with uppercase)
                if ident.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                    // It's a component - might have generic args
                    let mut expr = syn::ExprPath {
                        qself: None,
                        path: syn::parse_quote!(#ident),
                    }
                    .into_expr();
                    
                    // Handle generic arguments like Component<T>
                    // For now, keep it simple
                    Ok(HtmlTagName::Component(expr))
                } else {
                    Ok(HtmlTagName::BuiltIn(ident))
                }
            }
            None => Err("Unexpected end of input".to_string()),
        }
    }

    fn parse_attr(&mut self) -> Result<HtmlAttr, String> {
        // Skip whitespace
        while matches!(self.peek(), Some(t) if t.to_string().chars().all(|c| c.is_whitespace())) {
            self.advance();
        }

        match self.peek() {
            Some(token) if is_punct(token, '/') || is_punct(token, '>') => {
                return Err("Unexpected".to_string());
            }
            Some(token) if is_punct(token, '{') => {
                // Spread attribute: {...expr}
                self.advance();
                let expr = self.parse_expression()?;
                if !matches!(self.peek(), Some(t) if is_punct(t, '}')) {
                    return Err("Expected '}' after spread".to_string());
                }
                self.advance();
                Ok(HtmlAttr::Spread { expr })
            }
            _ => {
                // Normal attribute
                let name_token = self.advance()
                    .ok_or("Expected attribute name")?;
                let name = extract_ident(&name_token)
                    .ok_or("Expected identifier for attribute name")?;

                // Check for event handlers
                if name.starts_with("on") && name.len() > 2 {
                    let event_name = name[2..].to_lowercase();
                    // onClick -> on_click
                    let event_name = to_snake_case(&event_name);

                    if matches!(self.peek(), Some(t) if is_punct(t, '=')) {
                        self.advance();
                        let handler = self.parse_attr_value()?;
                        Ok(HtmlAttr::Event { name: event_name, handler })
                    } else {
                        Ok(HtmlAttr::Bool { name: event_name })
                    }
                } else {
                    // Normal attribute
                    let value = if matches!(self.peek(), Some(t) if is_punct(t, '=')) {
                        self.advance();
                        self.parse_attr_value()?
                    } else {
                        HtmlAttrValue::Str("true".to_string())
                    };
                    Ok(HtmlAttr::Normal { name, value })
                }
            }
        }
    }

    fn parse_attr_value(&mut self) -> Result<HtmlAttrValue, String> {
        match self.peek() {
            Some(token) if token.to_string().starts_with('"') || token.to_string().starts_with('\'') => {
                let value = self.parse_string()?;
                Ok(HtmlAttrValue::Str(value))
            }
            Some(token) if is_punct(token, '{') => {
                self.advance();
                let expr = self.parse_expression()?;
                if !matches!(self.peek(), Some(t) if is_punct(t, '}')) {
                    return Err("Expected '}'".to_string());
                }
                self.advance();
                Ok(HtmlAttrValue::Expr(expr))
            }
            _ => Err("Expected attribute value".to_string()),
        }
    }

    fn parse_expr_or_spread(&mut self) -> Result<HtmlNode, String> {
        // Check for spread: ...
        let start = self.pos;
        self.advance(); // {
        
        // Check next token
        if let Some(token) = self.peek() {
            if token.to_string() == "..." {
                self.advance();
                let expr = self.parse_expression()?;
                return Ok(HtmlNode::Expr(Expr::Verbatim(syn::ExprVerbatim {
                    tokens: quote! { ..(#expr) },
                })));
            }
        }
        
        // Regular expression
        self.pos = start;
        self.advance(); // {
        let expr = self.parse_expression()?;
        Ok(HtmlNode::Expr(expr))
    }

    fn parse_fragment(&mut self) -> Result<HtmlNode, String> {
        // Parse Fragment { children }
        self.advance(); // Fragment
        self.advance(); // {
        
        let mut children = Vec::new();
        loop {
            match self.peek() {
                None => break Err("Unclosed Fragment".to_string()),
                Some(t) if is_punct(t, '}') => {
                    self.advance();
                    break;
                }
                Some(t) if is_punct(t, '<') => {
                    children.push(self.parse_element()?);
                }
                Some(t) if is_punct(t, '{') => {
                    self.advance();
                    children.push(self.parse_expr_or_spread()?);
                    if !matches!(self.peek(), Some(t) if is_punct(t, '}')) {
                        return Err("Expected '}'".to_string());
                    }
                    self.advance();
                }
                _ => {
                    if let Some(text) = self.parse_text_content() {
                        if !text.trim().is_empty() {
                            children.push(HtmlNode::Text(text));
                        }
                    }
                }
            }
        }
        
        Ok(HtmlNode::Fragment(children))
    }

    fn parse_expression(&mut self) -> Result<Expr, String> {
        // Simplified: just collect tokens until }
        let mut tokens = Vec::new();
        let mut brace_depth = 1;
        
        while let Some(token) = self.advance() {
            if is_punct(&token, '{') {
                brace_depth += 1;
            }
            if is_punct(&token, '}') {
                brace_depth -= 1;
                if brace_depth == 0 {
                    break;
                }
            }
            tokens.push(token);
        }
        
        if tokens.is_empty() {
            return Ok(syn::parse_quote! { () });
        }
        
        let tokens: proc_macro2::TokenStream = tokens.into_iter().collect();
        syn::parse2(tokens).map_err(|e| e.to_string())
    }

    fn parse_string(&mut self) -> Result<String, String> {
        let quote_char = self.advance()
            .ok_or("Expected string")?
            .to_string()
            .chars()
            .next()
            .ok_or("Expected string")?;

        let mut value = String::new();
        while let Some(token) = self.advance() {
            let s = token.to_string();
            if s.starts_with(quote_char) && !s.ends_with('\\') {
                break;
            }
            value.push_str(&s);
        }
        
        Ok(value)
    }

    fn parse_text_content(&mut self) -> Option<String> {
        let mut text = String::new();
        
        while let Some(token) = self.peek() {
            let s = token.to_string();
            if s.contains('<') || s.contains('{') || s.contains('}') || s.contains('>') {
                break;
            }
            text.push_str(&s);
            self.advance();
        }
        
        if text.trim().is_empty() {
            None
        } else {
            Some(text)
        }
    }

    fn skip_closing_tag(&mut self) -> Result<(), String> {
        // Skip until >
        while let Some(token) = self.advance() {
            if is_punct(&token, '>') {
                return Ok(());
            }
        }
        Err("Expected >".to_string())
    }

    fn peek_n(&self, n: usize) -> Option<&proc_macro2::Token> {
        self.tokens.get(self.pos + n)
    }
}

// ============================================================================
// Code Generation
// ============================================================================

fn generate_vnode(node: &HtmlNode) -> proc_macro2::TokenStream {
    match node {
        HtmlNode::Text(text) => {
            let escaped = escape_string(text);
            quote! {
                ::runts::runtime::vdom::VNode::Text {
                    value: #escaped.into(),
                }
            }
        }
        HtmlNode::Expr(expr) => {
            // Expression result - might be VNode or primitive
            quote! {
                ::runts::runtime::vdom::to_vnode(#expr)
            }
        }
        HtmlNode::Element(elem) => {
            generate_element(elem)
        }
        HtmlNode::Fragment(children) => {
            let children: Vec<_> = children.iter().map(generate_vnode).collect();
            quote! {
                ::runts::runtime::vdom::VNode::Fragment(vec![#(#children),*])
            }
        }
    }
}

fn generate_element(elem: &HtmlElement) -> proc_macro2::TokenStream {
    let tag = match &elem.name {
        HtmlTagName::BuiltIn(name) => quote! { #name.into() },
        HtmlTagName::Component(expr) => {
            return generate_component_call(elem, expr);
        }
    };

    // Generate attributes
    let attrs = generate_attrs(&elem.attrs);
    
    // Generate children
    let children: Vec<_> = elem.children.iter().map(generate_vnode).collect();

    if elem.self_closing {
        quote! {
            ::runts::runtime::vdom::VNode::Element {
                tag: #tag,
                attrs: {
                    let mut map = ::std::collections::HashMap::new();
                    #(#attrs)*
                    map
                },
                children: vec![#(#children),*],
                events: ::std::collections::HashMap::new(),
            }
        }
    } else {
        quote! {
            ::runts::runtime::vdom::VNode::Element {
                tag: #tag,
                attrs: {
                    let mut map = ::std::collections::HashMap::new();
                    #(#attrs)*
                    map
                },
                children: vec![#(#children),*],
                events: ::std::collections::HashMap::new(),
            }
        }
    }
}

fn generate_component_call(elem: &HtmlElement, name: &Expr) -> proc_macro2::TokenStream {
    // Generate props from attributes
    let mut prop_assigns = Vec::new();
    let mut children_vec = Vec::new();
    let mut has_children = false;

    for attr in &elem.attrs {
        match attr {
            HtmlAttr::Normal { name, value } => {
                let key = syn::Ident::new(name, proc_macro2::Span::call_site());
                match value {
                    HtmlAttrValue::Str(s) => {
                        prop_assigns.push(quote! { #key: #s.into() });
                    }
                    HtmlAttrValue::Expr(expr) => {
                        prop_assigns.push(quote! { #key: #expr });
                    }
                }
            }
            HtmlAttr::Event { name, handler } => {
                let key = syn::Ident::new(name, proc_macro2::Span::call_site());
                prop_assigns.push(quote! { #key: Box::new(#handler) });
            }
            HtmlAttr::Spread { expr } => {
                // Spread: use ?
                prop_assigns.push(quote! { ..#expr });
            }
            HtmlAttr::Bool { name } => {
                let key = syn::Ident::new(name, proc_macro2::Span::call_site());
                prop_assigns.push(quote! { #key: true });
            }
        }
    }

    // Handle children
    for child in &elem.children {
        if let HtmlNode::Text(_) = child {
            // Text children go to `children` prop
            has_children = true;
        }
        children_vec.push(generate_vnode(child));
    }

    if has_children || !children_vec.is_empty() {
        prop_assigns.push(quote! { children: vec![#(#children_vec),*].into() });
    }

    quote! {
        #name {
            #(#prop_assigns),*
        }
    }
}

fn generate_attrs(attrs: &[HtmlAttr]) -> Vec<proc_macro2::TokenStream> {
    attrs.iter().map(|attr| {
        match attr {
            HtmlAttr::Normal { name, value } => {
                let key = map_attr_name(name);
                match value {
                    HtmlAttrValue::Str(s) => {
                        let escaped = escape_string(s);
                        quote! {
                            map.insert(#key.into(), ::runts::runtime::vdom::AttrValue::String(#escaped.into()));
                        }
                    }
                    HtmlAttrValue::Expr(expr) => {
                        quote! {
                            map.insert(#key.into(), ::runts::runtime::vdom::AttrValue::Value(::runts::runtime::vdom::to_attr_value(#expr)));
                        }
                    }
                }
            }
            HtmlAttr::Event { name, handler } => {
                let key = format!("on_{}", name);
                quote! {
                    // Event handler: #key = #handler
                    // map.insert(#key.into(), ::runts::runtime::vdom::AttrValue::Event(Box::new(#handler)));
                }
            }
            HtmlAttr::Spread { expr } => {
                quote! {
                    // Spread: merge #expr into map
                    if let ::runts::runtime::vdom::AttrValue::Object(obj) = ::runts::runtime::vdom::to_attr_value(#expr) {
                        map.extend(obj);
                    }
                }
            }
            HtmlAttr::Bool { name } => {
                let key = map_attr_name(name);
                quote! {
                    map.insert(#key.into(), ::runts::runtime::vdom::AttrValue::Bool(true));
                }
            }
        }
    }).collect()
}

fn map_attr_name(name: &str) -> String {
    match name {
        "class" => "class_name".to_string(),
        "for" => "for_id".to_string(),
        "type" => "input_type".to_string(),
        _ => name.to_string(),
    }
}

fn escape_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => result.push_str("\\\\"),
            '"' => result.push_str("\\\""),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c => result.push(c),
        }
    }
    result
}

fn to_snake_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut prev_was_upper = false;
    
    for c in s.chars() {
        if c.is_uppercase() {
            if !result.is_empty() && !prev_was_upper {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
            prev_was_upper = true;
        } else {
            result.push(c);
            prev_was_upper = false;
        }
    }
    
    result
}

// ============================================================================
// Token Utilities
// ============================================================================

fn is_punct(token: &proc_macro2::Token, expected: char) -> bool {
    matches!(token, proc_macro2::Token::Punct(p) if p.as_char() == Some(expected))
}

fn is_ident(token: &proc_macro2::Token, expected: &str) -> bool {
    match token {
        proc_macro2::Token::Ident(i) => i.to_string() == expected,
        _ => false,
    }
}

fn extract_ident(token: &proc_macro2::Token) -> Option<String> {
    match token {
        proc_macro2::Token::Ident(i) => Some(i.to_string()),
        _ => None,
    }
}

// ============================================================================
// Main Entry Point
// ============================================================================

pub fn html_macro(input: TokenStream) -> TokenStream {
    let tokens: Vec<_> = input.into_iter().collect();
    let mut parser = HtmlParser::new(tokens);

    match parser.parse_html() {
        Ok(node) => {
            let vnode = generate_vnode(&node);
            quote! {
                {
                    #vnode
                }
            }.into()
        }
        Err(e) => {
            syn::Error::new(proc_macro2::Span::call_site(), e)
                .to_compile_error()
                .into()
        }
    }
}
