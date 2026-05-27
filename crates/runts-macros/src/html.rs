//! html! procedural macro - JSX-like syntax for VNodes (syn 2.0 compatible)

use proc_macro2::{Delimiter, Literal, TokenStream, TokenTree};
use quote::quote;

/// Entry point for the `html!` macro
pub fn html_macro(input: TokenStream) -> TokenStream {
    let mut parser = Parser::new(input);
    match parser.parse_node() {
        Ok(node) => node.to_tokens(),
        Err(e) => {
            let msg = format!("html! macro error: {}", e);
            quote!(compile_error!(#msg))
        }
    }
}

// ============================================================================
// AST
// ============================================================================

enum Node {
    Element {
        tag: String,
        attrs: Vec<Attr>,
        children: Vec<Node>,
    },
    Component {
        name: String,
        attrs: Vec<Attr>,
        children: Vec<Node>,
    },
    Fragment {
        children: Vec<Node>,
    },
    Text(String),
    Expr(TokenStream),
}

struct Attr {
    name: String,
    value: AttrValue,
}

enum AttrValue {
    Lit(Literal),
    Expr(TokenStream),
    BoolTrue,
}

impl Node {
    fn to_tokens(&self) -> TokenStream {
        match self {
            Node::Text(s) => {
                quote!(::runts_lib::runtime::vdom::VNode::text(#s))
            }
            Node::Expr(ts) => {
                quote!(::runts_lib::runtime::vdom::into_vnode(#ts))
            }
            Node::Element { tag, attrs, children } => {
                let tag_lit = tag;
                let attr_tokens: Vec<TokenStream> = attrs.iter().filter_map(|a| {
                    let name = &a.name;
                    // Skip event handlers for SSR (they are handled client-side by islands)
                    if name.starts_with("on_") {
                        return None;
                    }
                    match &a.value {
                        AttrValue::Lit(lit) => {
                            Some(quote!(__el = __el.attr(#name, #lit);))
                        }
                        AttrValue::Expr(ts) => {
                            Some(quote!(__el = __el.attr(#name, #ts);))
                        }
                        AttrValue::BoolTrue => {
                            Some(quote!(__el = __el.attr(#name, true);))
                        }
                    }
                }).collect();
                let child_tokens: Vec<TokenStream> = children.iter().map(|c| {
                    let child_ts = c.to_tokens();
                    quote!(__el = __el.child({ #child_ts });)
                }).collect();
                quote!({
                    let mut __el = ::runts_lib::runtime::vdom::VNode::element(#tag_lit);
                    #(#attr_tokens)*
                    #(#child_tokens)*
                    __el
                })
            }
            Node::Component { name, attrs, children } => {
                let props_tokens: Vec<TokenStream> = attrs.iter().map(|a| {
                    let key = &a.name;
                    let val = match &a.value {
                        AttrValue::Lit(lit) => quote!(::serde_json::json!(#lit)),
                        AttrValue::Expr(ts) => quote!(::serde_json::json!(#ts)),
                        AttrValue::BoolTrue => quote!(::serde_json::json!(true)),
                    };
                    quote!(__p.insert(#key.to_string(), #val);)
                }).collect();
                let child_tokens: Vec<TokenStream> = children.iter().map(|c| {
                    let child_ts = c.to_tokens();
                    quote!(__children.push({ #child_ts });)
                }).collect();
                quote!({
                    let mut __p = ::std::collections::HashMap::<String, ::serde_json::Value>::new();
                    #(#props_tokens)*
                    let mut __children: Vec<::runts_lib::runtime::vdom::VNode> = Vec::new();
                    #(#child_tokens)*
                    ::runts_lib::runtime::vdom::VNode::Component {
                        name: #name.to_string(),
                        props: __p,
                        children: __children,
                    }
                })
            }
            Node::Fragment { children } => {
                let child_tokens: Vec<TokenStream> = children.iter().map(|c| {
                    let child_ts = c.to_tokens();
                    quote!(__children.push({ #child_ts });)
                }).collect();
                quote!({
                    let mut __children: Vec<::runts_lib::runtime::vdom::VNode> = Vec::new();
                    #(#child_tokens)*
                    ::runts_lib::runtime::vdom::VNode::Fragment { children: __children }
                })
            }
        }
    }
}

// ============================================================================
// Parser
// ============================================================================

struct Parser {
    tokens: Vec<TokenTree>,
    pos: usize,
}

impl Parser {
    fn new(stream: TokenStream) -> Self {
        let tokens: Vec<TokenTree> = stream.into_iter().collect();
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&TokenTree> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<TokenTree> {
        let t = self.tokens.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn parse_node(&mut self) -> Result<Node, String> {
        match self.peek() {
            Some(TokenTree::Literal(_)) => {
                let lit = self.advance().unwrap();
                Ok(Node::Text(lit.to_string().trim_matches('"').to_string()))
            }
            Some(TokenTree::Group(_)) => {
                let g = self.advance().unwrap();
                if let TokenTree::Group(g) = g {
                    Ok(Node::Expr(g.stream()))
                } else {
                    unreachable!()
                }
            }
            Some(TokenTree::Punct(p)) if p.as_char() == '<' => {
                self.parse_element_or_component()
            }
            other => Err(format!("Unexpected token in html! macro: {:?}", other)),
        }
    }

    fn parse_element_or_component(&mut self) -> Result<Node, String> {
        self.expect_punct('<')?;

        // Fragment: <>...</>
        if self.peek_punct('>') {
            self.advance(); // >
            let children = self.parse_children("")?;
            return Ok(Node::Fragment { children });
        }

        let tag = self.expect_ident()?;
        let is_component = tag.chars().next().map(|c| c.is_uppercase()).unwrap_or(false);

        let attrs = self.parse_attrs()?;

        // Self-closing?
        if self.peek_punct('/') {
            self.advance(); // /
            self.expect_punct('>')?;
            if is_component {
                return Ok(Node::Component { name: tag, attrs, children: vec![] });
            } else {
                return Ok(Node::Element { tag, attrs, children: vec![] });
            }
        }

        self.expect_punct('>')?;

        let children = self.parse_children(&tag)?;

        if is_component {
            Ok(Node::Component { name: tag, attrs, children })
        } else {
            Ok(Node::Element { tag, attrs, children })
        }
    }

    fn parse_attrs(&mut self) -> Result<Vec<Attr>, String> {
        let mut attrs = Vec::new();
        loop {
            if self.peek_punct('>') || self.peek_punct('/') {
                break;
            }

            // Spread: {...props}
            if let Some(TokenTree::Group(g)) = self.peek() {
                if g.delimiter() == Delimiter::Brace {
                    let ts = g.stream();
                    self.advance();
                    attrs.push(Attr {
                        name: String::new(),
                        value: AttrValue::Expr(quote!(#ts)),
                    });
                    continue;
                }
            }

            let name = match self.peek() {
                Some(TokenTree::Ident(i)) => {
                    let n = i.to_string();
                    self.advance();
                    n
                }
                _ => break,
            };

            // Check for = value
            if self.peek_punct('=') {
                self.advance(); // =
                let value = match self.peek() {
                    Some(TokenTree::Literal(lit)) => {
                        let l = lit.clone();
                        self.advance();
                        AttrValue::Lit(l)
                    }
                    Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => {
                        let ts = g.stream();
                        self.advance();
                        AttrValue::Expr(ts)
                    }
                    _ => return Err(format!("Expected attribute value after = for {}", name)),
                };
                attrs.push(Attr { name, value });
            } else {
                // Boolean attribute
                attrs.push(Attr { name, value: AttrValue::BoolTrue });
            }
        }
        Ok(attrs)
    }

    fn parse_children(&mut self, closing_tag: &str) -> Result<Vec<Node>, String> {
        let mut children = Vec::new();
        loop {
            // Check for closing tag: </tag> or </> for fragments
            if let (Some(TokenTree::Punct(p1)), Some(TokenTree::Punct(p2))) = (self.peek(), self.peek_n(1)) {
                if p1.as_char() == '<' && p2.as_char() == '/' {
                    self.advance(); // <
                    self.advance(); // /
                    // Fragment closing: </>
                    if closing_tag.is_empty() {
                        if !self.peek_punct('>') {
                            return Err(format!("Expected '/>' to close fragment"));
                        }
                        self.expect_punct('>')?;
                        break;
                    }
                    let close_name = self.expect_ident()?;
                    self.expect_punct('>')?;
                    if close_name != closing_tag {
                        return Err(format!("Mism closing tag: expected </{}>, found </{}>", closing_tag, close_name));
                    }
                    break;
                }
            }

            match self.peek() {
                None => return Err(format!("Unexpected end of input, expected </{}>", closing_tag)),
                Some(TokenTree::Punct(p)) if p.as_char() == '<' => {
                    children.push(self.parse_element_or_component()?);
                }
                Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => {
                    let ts = g.stream();
                    self.advance();
                    children.push(Node::Expr(ts));
                }
                Some(TokenTree::Literal(lit)) => {
                    let s = lit.to_string().trim_matches('"').to_string();
                    self.advance();
                    if !s.trim().is_empty() {
                        children.push(Node::Text(s));
                    }
                }
                Some(TokenTree::Ident(i)) => {
                    // Bare identifier as text (rare but possible)
                    let s = i.to_string();
                    self.advance();
                    children.push(Node::Text(s));
                }
                Some(_) => {
                    // Skip unknown tokens
                    self.advance();
                }
            }
        }
        Ok(children)
    }

    // ------------------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------------------

    fn peek_punct(&self, expected: char) -> bool {
        matches!(self.peek(), Some(TokenTree::Punct(p)) if p.as_char() == expected)
    }

    fn peek_n(&self, n: usize) -> Option<&TokenTree> {
        self.tokens.get(self.pos + n)
    }

    fn expect_punct(&mut self, expected: char) -> Result<(), String> {
        match self.advance() {
            Some(TokenTree::Punct(p)) if p.as_char() == expected => Ok(()),
            other => Err(format!("Expected '{}', found {:?}", expected, other)),
        }
    }

    fn expect_ident(&mut self) -> Result<String, String> {
        match self.advance() {
            Some(TokenTree::Ident(i)) => Ok(i.to_string()),
            other => Err(format!("Expected identifier, found {:?}", other)),
        }
    }
}
