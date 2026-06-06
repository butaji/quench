//! TokenStream-based Rust codegen using quote
//!
//! Generates proper Rust TokenStream instead of strings.
//! This enables compile-time validation and better error messages.
//!

use proc_macro2::TokenStream;
use quote::quote;

use super::{Expr, FunctionDecl, ObjectPatProp, Ownership, Pat, Stmt, Type, VariableDecl, VariableKind};

/// Quote-based code generator
#[allow(dead_code)]
pub struct QuoteCodegen;

#[allow(dead_code)]
impl QuoteCodegen {
    /// Generate a complete Rust module from HIR
    pub fn gen_module(&self, stmts: &[Stmt]) -> TokenStream {
        let items: Vec<TokenStream> = stmts
            .iter()
            .filter_map(|s| self.gen_stmt(s))
            .collect();
        
        quote! {
            #(#items)*
        }
    }
    
    /// Generate a function declaration
    pub fn gen_fn(&self, func: &FunctionDecl) -> TokenStream {
        let name = syn::Ident::new(&func.name, proc_macro2::Span::call_site());
        let params = self.gen_params(&func.params);
        let ret_type = self.gen_ret_type(&func.return_type, func.throws, &func.error_type);

        // Generate destructuring for pattern params at start of body
        let param_destructuring = self.gen_param_destructuring(&func.params);
        let body = self.gen_body_with_prefix(&func.body, param_destructuring);

        if func.is_async {
            quote! {
                pub async fn #name(#params) #ret_type {
                    #body
                }
            }
        } else {
            quote! {
                pub fn #name(#params) #ret_type {
                    #body
                }
            }
        }
    }

    /// Generate destructuring statements for params with patterns
    fn gen_param_destructuring(&self, params: &[super::Param]) -> Vec<TokenStream> {
        let mut decls = Vec::new();
        for param in params {
            if let Some(ref pattern) = param.pattern {
                let param_name = if param.name.is_empty() {
                    syn::Ident::new("__param", proc_macro2::Span::call_site())
                } else {
                    syn::Ident::new(&param.name, proc_macro2::Span::call_site())
                };
                let source = quote! { #param_name };
                let inner_decls = self.gen_pat(pattern, &source);
                for decl in inner_decls {
                    decls.push(quote! { let #decl; });
                }
            }
        }
        decls
    }

    /// Generate function body with additional statements at the start
    fn gen_body_with_prefix(&self, body: &Option<super::Block>, prefix: Vec<TokenStream>) -> TokenStream {
        match body {
            Some(b) => {
                let mut stmts: Vec<TokenStream> = prefix;
                stmts.extend(b.0.iter().filter_map(|s| self.gen_stmt(s)));
                quote! { #(#stmts)* }
            }
            None => {
                if prefix.is_empty() {
                    quote! { unimplemented!(); }
                } else {
                    quote! { #(#prefix)* }
                }
            }
        }
    }
    
    fn gen_params(&self, params: &[super::Param]) -> TokenStream {
        let params: Vec<TokenStream> = params
            .iter()
            .map(|p| self.gen_param(p))
            .collect();
        
        quote! { #(#params),* }
    }
    
    fn gen_param(&self, param: &super::Param) -> TokenStream {
        // When param has a pattern, use a placeholder name
        let name = if param.name.is_empty() {
            syn::Ident::new("__param", proc_macro2::Span::call_site())
        } else {
            syn::Ident::new(&param.name, proc_macro2::Span::call_site())
        };
        let ty = self.gen_param_type(param);

        quote! { #name: #ty }
    }
    
    fn gen_param_type(&self, param: &super::Param) -> TokenStream {
        let ty = param.type_.as_ref()
            .map(|t| self.gen_type(t))
            .unwrap_or_else(|| quote! { Value });
        
        match param.ownership {
            Ownership::Owned => ty,
            Ownership::Borrow => quote! { &#ty },
            Ownership::Mut => quote! { &mut #ty },
        }
    }
    
    fn gen_ret_type(&self, ret: &Option<Type>, throws: bool, error_type: &Option<Type>) -> TokenStream {
        let base = ret.as_ref()
            .map(|t| self.gen_type(t))
            .unwrap_or_else(|| quote! { () });
        
        if throws {
            let err = error_type.as_ref()
                .map(|t| self.gen_type(t))
                .unwrap_or_else(|| quote! { JsValue });
            
            quote! { -> Result<#base, #err> }
        } else {
            quote! { -> #base }
        }
    }
    
    fn gen_body(&self, body: &Option<super::Block>) -> TokenStream {
        match body {
            Some(b) => {
                let stmts: Vec<TokenStream> = b.0.iter()
                    .filter_map(|s| self.gen_stmt(s))
                    .collect();
                
                quote! {
                    #(#stmts)*
                }
            }
            None => quote! { unimplemented!(); },
        }
    }
    
    pub(crate) fn gen_type(&self, ty: &Type) -> TokenStream {
        use super::Type as T;
        match ty {
            T::String | T::Number | T::Boolean => self.gen_prim_type(ty),
            T::Void | T::Never | T::Undefined | T::Null | T::Unknown | T::Any | T::BigInt => self.gen_meta_type(ty),
            T::This => quote! { Self },
            T::Symbol => quote! { std::sync::Arc<std::fmt::Debug> },
            T::Query { .. } | T::Infer { .. } => quote! { Value },
            T::Readonly { inner } => self.gen_type(inner),
            T::Tuple { elements } => self.gen_tuple_type(elements),
            _ => self.gen_type_helper(ty),
        }
    }
}

include!("quote_codegen_types.inc");
include!("quote_codegen_stmts.inc");
include!("quote_codegen_exprs.inc");

impl QuoteCodegen {
    fn gen_jsx_expr(&self, jsx: &super::JSXExpr) -> TokenStream {
        use super::{JSXName, JSXChild, JSXAttr, JSXAttrValue};

        if matches!(jsx.opening.name, JSXName::Fragment) {
            let children = self.gen_jsx_children(&jsx.children);
            return quote! { VNode::fragment(vec![#(#children),*]) };
        }

        let name_str = match &jsx.opening.name {
            JSXName::Ident(s) => s.clone(),
            JSXName::Member { object, property } => format!("{}.{}", object, property),
            JSXName::Namespaced { ns, name } => format!("{}:{}", ns, name),
            JSXName::Dynamic(expr) => {
                let expr_tokens = self.gen_expr(expr);
                return quote! { VNode::element(#expr_tokens) };
            }
            JSXName::Fragment => {
                let children = self.gen_jsx_children(&jsx.children);
                return quote! { VNode::fragment(vec![#(#children),*]) };
            }
        };

        let is_component = !name_str.is_empty() && name_str.chars().next().map(|c| c.is_uppercase()).unwrap_or(false);

        if is_component {
            self.gen_jsx_component(&name_str, &jsx.opening.attrs, &jsx.children)
        } else {
            self.gen_jsx_element(&name_str, &jsx.opening.attrs, &jsx.children, jsx.opening.self_closing)
        }
    }

    fn gen_jsx_element(&self, tag: &str, attrs: &[super::JSXAttr], children: &[super::JSXChild], _self_closing: bool) -> TokenStream {
        use super::JSXAttrValue;
        let mut attr_calls: Vec<TokenStream> = Vec::new();
        for attr in attrs {
            match attr {
                super::JSXAttr::Attr { name, value } => self.gen_jsx_attr_call(name, value, &mut attr_calls),
                super::JSXAttr::Spread { expr } => {
                    let expr_tokens = self.gen_expr(expr);
                    attr_calls.push(quote! { /* spread: #expr_tokens */ });
                }
            }
        }
        let child_nodes: Vec<TokenStream> = self.gen_jsx_children(children);
        let mut result = quote! { VNode::element(#tag) };
        for attr_call in attr_calls { result = quote! { #result #attr_call }; }
        for child in child_nodes { result = quote! { #result .child(#child) }; }
        result
    }

    fn gen_jsx_attr_call(&self, name: &str, value: &Option<super::JSXAttrValue>, attr_calls: &mut Vec<TokenStream>) {
        use super::JSXAttrValue;
        let key = name.to_string();
        match value {
            Some(JSXAttrValue::String(s)) => attr_calls.push(quote! { .attr(#key, #s) }),
            Some(JSXAttrValue::Expr(expr)) => {
                let val = self.gen_expr(expr);
                attr_calls.push(quote! { .attr(#key, #val) });
            }
            Some(JSXAttrValue::Empty) | None => attr_calls.push(quote! { .attr(#key, true) }),
        }
    }

    fn gen_jsx_component(&self, name: &str, attrs: &[super::JSXAttr], children: &[super::JSXChild]) -> TokenStream {
        let props_fields = self.gen_jsx_props(attrs);
        let child_nodes: Vec<TokenStream> = self.gen_jsx_children(children);
        self.gen_component_render(name, &props_fields, &child_nodes)
    }

    fn gen_jsx_props(&self, attrs: &[super::JSXAttr]) -> Vec<TokenStream> {
        let mut props_fields: Vec<TokenStream> = Vec::new();
        for attr in attrs {
            match attr {
                super::JSXAttr::Attr { name: prop_name, value } => {
                    let key = syn::Ident::new(prop_name, proc_macro2::Span::call_site());
                    self.gen_jsx_attr_value(&key, value, &mut props_fields);
                }
                super::JSXAttr::Spread { expr } => {
                    let expr_tokens = self.gen_expr(expr);
                    props_fields.push(quote! { /* spread: #expr_tokens */ });
                }
            }
        }
        props_fields
    }

    fn gen_jsx_attr_value(&self, key: &syn::Ident, value: &Option<super::JSXAttrValue>, props: &mut Vec<TokenStream>) {
        use super::JSXAttrValue;
        match value {
            Some(JSXAttrValue::String(s)) => props.push(quote! { #key: #s.to_string() }),
            Some(JSXAttrValue::Expr(expr)) => {
                let val = self.gen_expr(expr);
                props.push(quote! { #key: #val });
            }
            Some(JSXAttrValue::Empty) | None => props.push(quote! { #key: true }),
        }
    }

    fn gen_component_render(&self, name: &str, props_fields: &[TokenStream], child_nodes: &[TokenStream]) -> TokenStream {
        let component_name_str = name.to_string();
        let props_name_str = format!("{}Props", name);
        if props_fields.is_empty() && child_nodes.is_empty() {
            quote! { #component_name_str::render(#props_name_str {}) }
        } else if props_fields.is_empty() {
            quote! { #component_name_str::render(#props_name_str { children: VNode::fragment(vec![#(#child_nodes),*]) }) }
        } else if child_nodes.is_empty() {
            quote! { #component_name_str::render(#props_name_str { #(#props_fields),* }) }
        } else {
            quote! { #component_name_str::render(#props_name_str { #(#props_fields),*, children: VNode::fragment(vec![#(#child_nodes),*]) }) }
        }
    }

    fn gen_jsx_children(&self, children: &[super::JSXChild]) -> Vec<TokenStream> {
        use super::JSXChild;
        children.iter().filter_map(|child| {
            match child {
                JSXChild::Text(s) => Some(quote! { VNode::text(#s) }),
                JSXChild::Expr(expr) => {
                    let expr_tokens = self.gen_expr(expr);
                    Some(expr_tokens)
                }
                JSXChild::JSX(jsx) => Some(self.gen_jsx_expr(jsx)),
                JSXChild::Fragment { children: frag_children } => {
                    let inner = self.gen_jsx_children(frag_children);
                    Some(quote! { VNode::fragment(vec![#(#inner),*]) })
                }
                JSXChild::Spread { expr } => {
                    let expr_tokens = self.gen_expr(expr);
                    Some(quote! { /* spread: #expr_tokens */ })
                }
            }
        }).collect()
    }


    fn gen_update_expr(&self, op: &super::UpdateOp, arg: &Expr, prefix: bool) -> TokenStream {
        use super::UpdateOp as U;
        let val = self.gen_expr(arg);
        match op {
            U::PlusPlus if prefix => quote! { { let __v: f64 = #val + 1.0; #val = __v; __v } },
            U::PlusPlus => quote! { { let __v = #val; #val += 1.0; __v } },
            U::MinusMinus if prefix => quote! { { let __v: f64 = #val - 1.0; #val = __v; __v } },
            U::MinusMinus => quote! { { let __v = #val; #val -= 1.0; __v } },
        }
    }

    fn gen_bin_expr(&self, op: &super::BinaryOp, left: &Expr, right: &Expr) -> TokenStream {
        use super::BinaryOp as B;
        let lhs = self.gen_expr(left);
        let rhs = self.gen_expr(right);

        if matches!(op, B::Instanceof) { return quote! { false }; }
        if matches!(op, B::In) { return quote! { #rhs.contains_key(&#lhs) }; }

        if matches!(op, B::Add) && self.is_string_expr(left) { return quote! { format!(concat!(#lhs, "{}"), #rhs) }; }
        if matches!(op, B::Add) && self.is_string_expr(right) { return quote! { format!("{}{}", #lhs, #rhs) }; }

        let op_tok = self.bin_op(op);
        quote! { #lhs #op_tok #rhs }
    }

    fn is_string_expr(&self, expr: &Expr) -> bool {
        matches!(expr, Expr::String(_))
    }

    fn gen_call_expr(&self, callee: &Expr, arguments: &[Expr]) -> TokenStream {
        if let Expr::Ident { name } = callee {
            if let Some(rust_code) = self.gen_global_fn(name, arguments) {
                return rust_code;
            }
        }
        if let Expr::StaticMember { obj, property } = callee {
            if let Some(code) = self.gen_console_call(obj.as_ref(), property, arguments) {
                return code;
            }
        }
        let callee = self.gen_expr(callee);
        let args: Vec<_> = arguments.iter().map(|a| { let arg = self.gen_expr(a); if matches!(a, Expr::String(_)) { quote! { #arg.to_string() } } else { arg } }).collect();
        quote! { #callee(#(#args),*) }
    }

    fn gen_global_fn(&self, name: &str, arguments: &[Expr]) -> Option<TokenStream> {
        match name {
            "fetch" => { let url = arguments.first().map(|a| self.gen_expr(a)).unwrap_or_else(|| quote! { String::new() }); Some(quote! { reqwest::get(#url).await? }) }
            "setTimeout" => { let dur = arguments.get(1).map(|a| self.gen_expr(a)).unwrap_or_else(|| quote! { 0 }); Some(quote! { tokio::time::sleep(std::time::Duration::from_millis(#dur as u64)).await }) }
            "setInterval" => { let dur = arguments.get(1).map(|a| self.gen_expr(a)).unwrap_or_else(|| quote! { 0 }); Some(quote! { tokio::time::interval(std::time::Duration::from_millis(#dur as u64)) }) }
            _ => None,
        }
    }

    fn gen_console_call(&self, obj: &Expr, property: &str, arguments: &[Expr]) -> Option<TokenStream> {
        if let Expr::Ident { name } = obj { if name != "console" { return None; } } else { return None; }
        match property { "log" | "error" | "info" | "table" | "warn" => Some(self.gen_console_output(property, arguments)), "assert" => Some(self.gen_console_assert(arguments)), _ => None }
    }

    fn gen_console_output(&self, method: &str, args: &[Expr]) -> TokenStream {
        let is_err = method == "error" || method == "warn";
        let gen_args: Vec<_> = args.iter().map(|a| self.gen_expr(a)).collect();
        if args.len() == 1 { if is_err { syn::parse_quote! { eprintln!("{}", #(#gen_args),*) } } else { syn::parse_quote! { println!("{}", #(#gen_args),*) } } }
        else { let fmts: Vec<_> = args.iter().map(|_| quote! { "{}" }).collect(); if is_err { syn::parse_quote! { eprintln!(#(#fmts),*, #(#gen_args),*) } } else { syn::parse_quote! { println!(#(#fmts),*, #(#gen_args),*) } } }
    }

    fn gen_console_assert(&self, args: &[Expr]) -> TokenStream {
        let gen_args: Vec<_> = args.iter().map(|a| self.gen_expr(a)).collect();
        if args.len() >= 2 { let cond = gen_args.first().unwrap(); let msg = gen_args.get(1).unwrap(); quote! { assert!(#cond, "{}", #msg) } }
        else if args.len() == 1 { quote! { assert!(#(gen_args.first().unwrap())) } } else { quote! { () } }
    }

    fn gen_assign_expr(&self, op: &super::AssignOp, left: &Expr, right: &Expr) -> TokenStream {
        use super::AssignOp as A;
        let lhs = self.gen_expr(left);
        let rhs = self.gen_expr(right);
        match op {
            A::Assign => quote! { { let __v = #rhs; #lhs = __v; __v } },
            A::AddAssign => quote! { { let __v = #lhs + #rhs; #lhs = __v; __v } },
            A::SubAssign => quote! { { let __v = #lhs - #rhs; #lhs = __v; __v } },
            A::MulAssign => quote! { { let __v = #lhs * #rhs; #lhs = __v; __v } },
            A::DivAssign => quote! { { let __v = #lhs / #rhs; #lhs = __v; __v } },
            _ => quote! { { let __v = #rhs; #lhs = __v; __v } },
        }
    }

    fn bin_op(&self, op: &super::BinaryOp) -> TokenStream {
        use super::BinaryOp as B;
        match op {
            B::Add | B::Sub | B::Mul => Self::bin_arith(op),
            B::Div | B::DivStrict | B::Mod | B::Exp => Self::bin_mul(op),
            B::BitXor | B::BitAnd | B::BitOr => Self::bin_bit(op),
            B::Shl | B::Shr | B::UShr => Self::bin_shift(op),
            B::Eq | B::StrictEq | B::Neq | B::StrictNeq => Self::bin_eq(op),
            B::Lt | B::Lte | B::Gt | B::Gte => Self::bin_cmp(op),
            B::Instanceof | B::In => Self::bin_js_op(op),
            B::LogicalAnd | B::LogicalOr | B::NullishCoalescing => Self::bin_logical(op),
        }
    }

    fn bin_arith(op: &super::BinaryOp) -> TokenStream { use super::BinaryOp as B; match op { B::Add => quote! { + }, B::Sub => quote! { - }, B::Mul => quote! { * }, _ => quote! { 0 } } }
    fn bin_mul(op: &super::BinaryOp) -> TokenStream { use super::BinaryOp as B; match op { B::Div | B::DivStrict => quote! { / }, B::Mod => quote! { % }, B::Exp => quote! { powf() }, _ => quote! { 1 } } }
    fn bin_bit(op: &super::BinaryOp) -> TokenStream { use super::BinaryOp as B; match op { B::BitXor => quote! { ^ }, B::BitAnd => quote! { & }, B::BitOr => quote! { | }, _ => quote! { 0 } } }
    fn bin_shift(op: &super::BinaryOp) -> TokenStream { use super::BinaryOp as B; match op { B::Shl => quote! { << }, B::Shr => quote! { >> }, B::UShr => quote! { >>> }, _ => quote! { 0 } } }
    fn bin_eq(op: &super::BinaryOp) -> TokenStream { use super::BinaryOp as B; match op { B::Eq => quote! { == }, B::StrictEq => quote! { === }, B::Neq => quote! { != }, B::StrictNeq => quote! { !== }, _ => quote! { false } } }
    fn bin_cmp(op: &super::BinaryOp) -> TokenStream { use super::BinaryOp as B; match op { B::Lt => quote! { < }, B::Lte => quote! { <= }, B::Gt => quote! { > }, B::Gte => quote! { >= }, _ => quote! { false } } }
    fn bin_js_op(op: &super::BinaryOp) -> TokenStream { use super::BinaryOp as B; match op { B::Instanceof => quote! { instanceof }, B::In => quote! { in }, _ => quote! { 0 } } }
    fn bin_logical(op: &super::BinaryOp) -> TokenStream { use super::BinaryOp as B; match op { B::LogicalAnd => quote! { && }, B::LogicalOr => quote! { || }, B::NullishCoalescing => quote! { ?? }, _ => quote! { false } } }
}

impl Default for QuoteCodegen {
    fn default() -> Self {
        Self
    }
}

#[cfg(test)]
mod tests {
    include!("quote_codegen_tests.inc");
}
