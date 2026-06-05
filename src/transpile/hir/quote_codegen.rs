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

    fn gen_type_helper(&self, ty: &Type) -> TokenStream {
        use super::Type as T;
        match ty {
            T::Array { elem } | T::Ref { name: _, generics: _ } => self.gen_array_type(elem),
            T::Object { members } => self.gen_object_type(members),
            T::Union { types } | T::Intersection { types } => self.gen_union_type(types),
            T::Literal { kind, value } => self.gen_literal_type(kind, value),
            T::Template { parts, values } => self.gen_template_type(parts, values),
            T::Function { params, ret } => self.gen_fn_type(params, ret),
            T::Index { obj, index } => self.gen_index_type(obj, index),
            T::Mapped { from, to } => self.gen_mapped_type(from, to),
            T::Conditional { check, extends, true_type, false_type } => {
                self.gen_conditional_type(check, extends, true_type, false_type)
            }
            T::Partial { inner } | T::Required { inner } => self.gen_partial_type(inner),
            T::Pick { inner, keys } | T::Omit { inner, keys } => self.gen_pick_type(inner, keys),
            T::Record { key, value } => self.gen_record_type(key, value),
            T::KeyOf { inner } => self.gen_keyof_type(inner),
            T::ReturnType { inner } | T::Parameters { inner } => self.gen_return_type(inner),
            _ => quote! { Value },
        }
    }

    fn gen_tuple_type(&self, elements: &[TypeTupleElement]) -> TokenStream {
        let types: Vec<_> = elements.iter().map(|e| self.gen_type(&e.type_)).collect();
        quote! { (#(#types),*) }
    }

    fn gen_index_type(&self, obj: &Box<Type>, index: &Box<Type>) -> TokenStream {
        let obj_t = self.gen_type(obj);
        let index_t = self.gen_type(index);
        quote! { std::collections::HashMap<#obj_t, #index_t> }
    }

    fn gen_mapped_type(&self, from: &Box<Type>, to: &Box<Type>) -> TokenStream {
        let from_t = self.gen_type(from);
        let to_t = self.gen_type(to);
        quote! { std::collections::HashMap<#from_t, #to_t> }
    }

    fn gen_conditional_type(&self, check: &Box<Type>, extends: &Box<Type>, true_type: &Box<Type>, false_type: &Box<Type>) -> TokenStream {
        let check_t = self.gen_type(check);
        let extends_t = self.gen_type(extends);
        let true_t = self.gen_type(true_type);
        let false_t = self.gen_type(false_type);
        quote! { if #check_t: #extends_t { #true_t } else { #false_t } }
    }

    fn gen_record_type(&self, key: &Box<Type>, value: &Box<Type>) -> TokenStream {
        let key_t = self.gen_type(key);
        let value_t = self.gen_type(value);
        quote! { std::collections::HashMap<#key_t, #value_t> }
    }

    fn gen_partial_type(&self, inner: &Box<Type>) -> TokenStream {
        use super::Type as T;
        match inner.as_ref() {
            T::Object { members } => {
                let fields: Vec<_> = members.iter()
                    .map(|m| {
                        let name = syn::Ident::new(&m.key, proc_macro2::Span::call_site());
                        let ty = self.gen_type(&m.type_);
                        quote! { pub #name: Option<#ty> }
                    })
                    .collect();
                quote! { { #(#fields);* } }
            }
            _ => quote! { Value },
        }
    }

    fn gen_required_type(&self, inner: &Box<Type>) -> TokenStream {
        self.gen_type(inner)
    }

    fn gen_pick_type(&self, inner: &Box<Type>, keys: &[String]) -> TokenStream {
        use super::Type as T;
        match inner.as_ref() {
            T::Object { members } => {
                let fields: Vec<_> = members.iter()
                    .filter(|m| keys.contains(&m.key))
                    .map(|m| {
                        let name = syn::Ident::new(&m.key, proc_macro2::Span::call_site());
                        let ty = self.gen_type(&m.type_);
                        quote! { pub #name: #ty }
                    })
                    .collect();
                if fields.is_empty() {
                    quote! { Value }
                } else {
                    quote! { { #(#fields);* } }
                }
            }
            _ => quote! { Value },
        }
    }

    fn gen_omit_type(&self, inner: &Box<Type>, keys: &[String]) -> TokenStream {
        use super::Type as T;
        match inner.as_ref() {
            T::Object { members } => {
                let fields: Vec<_> = members.iter()
                    .filter(|m| !keys.contains(&m.key))
                    .map(|m| {
                        let name = syn::Ident::new(&m.key, proc_macro2::Span::call_site());
                        let ty = self.gen_type(&m.type_);
                        quote! { pub #name: #ty }
                    })
                    .collect();
                if fields.is_empty() {
                    quote! { Value }
                } else {
                    quote! { { #(#fields);* } }
                }
            }
            _ => quote! { Value },
        }
    }

    fn gen_keyof_type(&self, inner: &Box<Type>) -> TokenStream {
        use super::Type as T;
        match inner.as_ref() {
            T::Object { members } => {
                let variants: Vec<_> = members.iter()
                    .map(|m| {
                        let variant = syn::Ident::new(&m.key, proc_macro2::Span::call_site());
                        quote! { #variant }
                    })
                    .collect();
                quote! { enum { #(#variants),* } }
            }
            _ => quote! { Value },
        }
    }

    fn gen_return_type(&self, inner: &Box<Type>) -> TokenStream {
        use super::Type as T;
        match inner.as_ref() {
            T::Function { params: _, ret } => self.gen_type(ret),
            _ => quote! { Value },
        }
    }

    fn gen_parameters_type(&self, inner: &Box<Type>) -> TokenStream {
        use super::Type as T;
        match inner.as_ref() {
            T::Function { params, ret: _ } => {
                let param_types: Vec<_> = params.iter()
                    .map(|p| self.gen_type(p))
                    .collect();
                quote! { (#(#param_types),*) }
            }
            _ => quote! { Value },
        }
    }

    fn gen_union_type(&self, types: &[Type]) -> TokenStream {
        if types.is_empty() {
            return quote! { Value };
        }
        let variants: Vec<TokenStream> = types.iter()
            .enumerate()
            .map(|(i, t)| self.gen_union_variant(i, t))
            .collect();
        quote! { enum { #(#variants),* } }
    }

    fn gen_union_variant(&self, index: usize, ty: &Type) -> TokenStream {
        use super::Type as T;
        match ty {
            T::Ref { name, generics } => {
                let name_id = syn::Ident::new(name, proc_macro2::Span::call_site());
                if generics.is_empty() {
                    quote! { #name_id }
                } else {
                    let gs: Vec<_> = generics.iter().map(|g| self.gen_type(g)).collect();
                    quote! { #name_id<#(#gs),*> }
                }
            }
            T::Object { members } => {
                let fields: Vec<_> = members.iter()
                    .map(|m| {
                        let key = syn::Ident::new(&m.key, proc_macro2::Span::call_site());
                        let ty = self.gen_type(&m.type_);
                        quote! { #key: #ty }
                    })
                    .collect();
                let variant_ident = syn::Ident::new(&format!("Variant{}", index), proc_macro2::Span::call_site());
                quote! { #variant_ident { #(#fields),* } }
            }
            T::Literal { kind, value } => {
                let variant_name = syn::Ident::new(&format!("{:?}{}", kind, value), proc_macro2::Span::call_site());
                quote! { #variant_name }
            }
            _ => {
                let variant_ident = syn::Ident::new(&format!("Variant{}", index), proc_macro2::Span::call_site());
                quote! { #variant_ident }
            }
        }
    }

    fn gen_intersection_type(&self, types: &[Type]) -> TokenStream {
        if types.is_empty() {
            return quote! { Value };
        }
        let all_fields: Vec<TokenStream> = types.iter()
            .filter_map(|t| {
                if let super::Type::Object { members } = t {
                    Some(members.iter().map(|m| {
                        let key = syn::Ident::new(&m.key, proc_macro2::Span::call_site());
                        let ty = self.gen_type(&m.type_);
                        quote! { pub #key: #ty }
                    }).collect::<Vec<_>>())
                } else {
                    None
                }
            })
            .flatten()
            .collect();

        if all_fields.is_empty() {
            let type_strs: Vec<_> = types.iter().map(|t| self.gen_type(t)).collect();
            quote! { (#(#type_strs),*) }
        } else {
            quote! { struct { #(#all_fields);* } }
        }
    }

    fn gen_literal_type(&self, kind: &super::LiteralKind, value: &str) -> TokenStream {
        match kind {
            super::LiteralKind::String => {
                let s = value.to_string();
                quote! { #s }
            }
            super::LiteralKind::Number => {
                if let Ok(n) = value.parse::<f64>() {
                    quote! { #n }
                } else {
                    quote! { Value }
                }
            }
            super::LiteralKind::Boolean => {
                if value == "true" {
                    quote! { true }
                } else {
                    quote! { false }
                }
            }
            super::LiteralKind::BigInt => {
                if let Ok(n) = value.parse::<i64>() {
                    quote! { #n }
                } else {
                    quote! { Value }
                }
            }
        }
    }

    fn gen_template_type(&self, parts: &[super::TemplatePart], values: &[Type]) -> TokenStream {
        let mut tokens = TokenStream::new();
        for (i, part) in parts.iter().enumerate() {
            match part {
                super::TemplatePart::String { value: s } => {
                    let s = s.to_string();
                    tokens.extend(quote! { #s });
                }
                super::TemplatePart::Type { value: t } => {
                    tokens.extend(self.gen_type(t));
                }
            }
            if i < values.len() {
                tokens.extend(self.gen_type(&values[i]));
            }
        }
        if tokens.is_empty() {
            quote! { String::new() }
        } else {
            tokens
        }
    }

    fn gen_fn_type(&self, params: &[Type], ret: &Box<Type>) -> TokenStream {
        let ps: Vec<_> = params.iter().map(|p| self.gen_type(p)).collect();
        let r = self.gen_type(ret);
        quote! { fn(#(#ps),*) -> #r }
    }
    
    fn gen_prim_type(&self, ty: &Type) -> TokenStream {
        use super::Type as T;
        match ty {
            T::String => quote! { String },
            T::Number => quote! { f64 },
            T::Boolean => quote! { bool },
            _ => quote! { Value },
        }
    }
    
    fn gen_meta_type(&self, ty: &Type) -> TokenStream {
        use super::Type as T;
        match ty {
            T::Void | T::Never => quote! { () },
            T::Undefined | T::Null | T::Unknown | T::Any => quote! { Value },
            T::BigInt => quote! { i64 },
            _ => quote! { Value },
        }
    }
    
    fn gen_array_type(&self, elem: &Box<Type>) -> TokenStream {
        let inner = self.gen_type(elem);
        quote! { Vec<#inner> }
    }
    
    fn gen_ref_type(&self, name: &str, generics: &[Type]) -> TokenStream {
        let name = syn::Ident::new(name, proc_macro2::Span::call_site());
        if generics.is_empty() {
            quote! { #name }
        } else {
            let generics: Vec<_> = generics.iter().map(|g| self.gen_type(g)).collect();
            quote! { #name<#(#generics),*> }
        }
    }
    
    fn gen_object_type(&self, members: &[super::TypeMember]) -> TokenStream {
        if members.is_empty() {
            return quote! { serde_json::Value };
        }
        let fields: Vec<_> = members.iter()
            .map(|m| {
                let name = syn::Ident::new(&m.key, proc_macro2::Span::call_site());
                let ty = self.gen_type(&m.type_);
                quote! { pub #name: #ty }
            })
            .collect();
        quote! { { #(#fields);* } }
    }
    
    pub(crate) fn gen_stmt(&self, stmt: &Stmt) -> Option<TokenStream> {
        use super::Stmt as S;
        match stmt {
            S::Empty => Some(quote! {}),
            S::Expr { expr } => Some(self.gen_expr_stmt(expr)),
            S::Return { arg } => Some(self.gen_return(arg)),
            S::If { test, consequent, alternate } => {
                let alt_stmt = alternate.as_ref().map(|b| b.as_ref());
                Some(self.gen_if(test, consequent, alt_stmt))
            }
            S::Switch { discriminant, cases } => Some(self.gen_switch(discriminant, cases)),
            S::For { init, test, update, body } => Some(self.gen_for(init, test, update, body)),
            S::ForIn { left, right, body, .. } => Some(self.gen_for_in(left, right, body)),
            S::ForOf { left, right, body, is_await } => Some(self.gen_for_of(left, right, body, *is_await)),
            S::While { test, body } => Some(self.gen_while(test, body)),
            S::DoWhile { body, test } => Some(self.gen_do_while(body, test)),
            S::Break { label } => Some(self.gen_break(label)),
            S::Continue { label } => Some(self.gen_continue(label)),
            S::Throw { arg } => Some(self.gen_throw(arg)),
            S::Try { block, handler, finalizer } => Some(self.gen_try(block, handler, finalizer)),
            S::Block { stmts } => Some(self.gen_block(stmts)),
            S::Labeled { label, body } => Some(self.gen_labeled(label, body)),
            S::With { obj, body } => Some(self.gen_with(obj, body)),
            S::FunctionDecl(func) => Some(self.gen_fn(func)),
            S::Class(_) => None, // Class codegen not yet implemented
            S::Variable(var) => self.gen_var_decl(var),
            S::ExportNamed { specifiers } => Some(self.gen_export_named(specifiers)),
            S::ExportDefault { expr } => Some(self.gen_export_default(expr)),
            S::ImportNamed { source, specifiers } => Some(self.gen_import_named(source, specifiers)),
            S::ImportDefault { source, local } => Some(self.gen_import_default(source, local)),
        }
    }

    fn gen_expr_stmt(&self, expr: &Expr) -> TokenStream {
        let expr = self.gen_expr(expr);
        quote! { #expr; }
    }

    fn gen_return(&self, arg: &Option<Expr>) -> TokenStream {
        match arg {
            Some(e) => {
                let expr = self.gen_expr(e);
                quote! { return #expr; }
            }
            None => quote! { return; },
        }
    }

    fn gen_export_named(&self, specifiers: &[super::Export]) -> TokenStream {
        use super::Export as E;
        let items: Vec<TokenStream> = specifiers.iter().map(|spec| {
            match spec {
                E::Named { name } => { let ident = syn::Ident::new(name, proc_macro2::Span::call_site()); quote! { #ident } }
                E::NamedWithValue { name, value } => { let ident = syn::Ident::new(name, proc_macro2::Span::call_site()); let val = self.gen_expr(value); quote! { #ident = #val } }
                E::ReExport { source, names } => { let mod_path = self.module_path(source); let idents: Vec<_> = names.iter().map(|n| syn::Ident::new(n, proc_macro2::Span::call_site())).collect(); quote! { pub use #mod_path::{#(#idents),*}; } }
                E::All { source } => { let mod_path = self.module_path(source); quote! { pub use #mod_path::*; } }
                E::Default { expr } => { let val = self.gen_expr(expr); quote! { #val } }
            }
        }).collect();

        if items.len() == 1 {
            let item = &items[0];
            if matches!(specifiers[0], E::Named { .. }) { quote! { pub use #item; } } else { quote! { #item } }
        } else {
            let uses: Vec<TokenStream> = specifiers.iter().filter_map(|spec| {
                if let E::Named { name } = spec { let ident = syn::Ident::new(name, proc_macro2::Span::call_site()); Some(quote! { pub use #ident; }) } else { None }
            }).collect();
            quote! { #(#uses)* }
        }
    }

    fn gen_export_default(&self, expr: &Expr) -> TokenStream {
        let val = self.gen_expr(expr);
        match expr {
            Expr::Function(_) => {
                // export default function -> fn default() { ... }
                // The function already has its own name
                quote! { #val }
            }
            Expr::ArrowFunction { .. } => {
                // export default arrow -> fn default() { ... }
                quote! { fn default() { #val } }
            }
            _ => {
                // export default expr -> pub const default = expr;
                quote! { pub const default = #val; }
            }
        }
    }

    fn gen_import_named(&self, source: &str, specifiers: &[super::ImportSpecifier]) -> TokenStream {
        use super::ImportSpecifier as I;
        let mod_path = self.module_path(source);

        let items: Vec<TokenStream> = specifiers.iter().map(|spec| {
            match spec {
                I::Named { name, alias } => {
                    let orig = syn::Ident::new(name, proc_macro2::Span::call_site());
                    match alias {
                        Some(a) => {
                            let alias_ident = syn::Ident::new(a, proc_macro2::Span::call_site());
                            quote! { #orig as #alias_ident }
                        }
                        None => quote! { #orig },
                    }
                }
                I::Default { name } => {
                    let ident = syn::Ident::new(name, proc_macro2::Span::call_site());
                    quote! { #ident }
                }
                I::Namespace { name } => {
                    let ident = syn::Ident::new(name, proc_macro2::Span::call_site());
                    quote! { * as #ident }
                }
            }
        }).collect();

        quote! { use #mod_path::{#(#items),*}; }
    }

    fn gen_import_default(&self, source: &str, local: &str) -> TokenStream {
        let mod_path = self.module_path(source);
        let ident = syn::Ident::new(local, proc_macro2::Span::call_site());
        quote! { use #mod_path::#ident; }
    }

    /// Convert a JS module path to a Rust module path
    /// e.g., "react" -> "::runts::react", "./utils/helper" -> "::runts::helper"
    fn module_path(&self, source: &str) -> syn::Path {
        // For relative paths, extract the last component
        let module_name = if source.starts_with('.') {
            source.split('/').last().unwrap_or(source)
        } else {
            source
        };

        syn::Ident::new(module_name, proc_macro2::Span::call_site()).into()
    }

    fn gen_if(&self, test: &Expr, cons: &Box<Stmt>, alt: Option<&Stmt>) -> TokenStream {
        let test = self.gen_expr(test);
        let cons = self.gen_block_stmt(cons);

        match alt {
            Some(a) => {
                let alt = self.gen_block_stmt(&Box::new(a.clone()));
                quote! {
                    if #test {
                        #cons
                    } else {
                        #alt
                    }
                }
            }
            None => quote! {
                if #test {
                    #cons
                }
            },
        }
    }

    fn gen_switch(&self, discriminant: &Expr, cases: &[super::SwitchCase]) -> TokenStream {
        let disc = self.gen_expr(discriminant);
        let arms: Vec<TokenStream> = cases
            .iter()
            .map(|case| {
                let test = case.test.as_ref().map(|t| self.gen_expr(t));
                let consequent: Vec<TokenStream> = case.consequent
                    .iter()
                    .filter_map(|s| self.gen_stmt(s))
                    .collect();
                match test {
                    Some(t) => quote! {
                        #t => {
                            #(#consequent)*
                        }
                    },
                    None => quote! {
                        _ => {
                            #(#consequent)*
                        }
                    },
                }
            })
            .collect();
        quote! {
            match #disc {
                #(#arms)*
            }
        }
    }

    fn gen_for(&self, init: &Option<super::ForInit>, test: &Option<Expr>, update: &Option<Expr>, body: &Box<Stmt>) -> TokenStream {
        let init_tokens = self.gen_for_init(init);
        let test_token = test.as_ref().map(|t| self.gen_expr(t));
        let update_token = update.as_ref().map(|u| self.gen_expr(u));
        let body_token = self.gen_block_stmt(body);
        quote! {
            for #init_tokens #test_token; #update_token {
                #body_token
            }
        }
    }

    fn gen_for_in(&self, left: &super::ForInit, right: &Expr, body: &Box<Stmt>) -> TokenStream {
        let left_token = self.gen_for_init(&Some(left.clone()));
        let right_token = self.gen_expr(right);
        let body_token = self.gen_block_stmt(body);
        quote! {
            for #left_token in #right_token {
                #body_token
            }
        }
    }

    fn gen_for_of(&self, left: &super::ForInit, right: &Expr, body: &Box<Stmt>, is_await: bool) -> TokenStream {
        let left_token = self.gen_for_init(&Some(left.clone()));
        let right_token = self.gen_expr(right);
        let body_token = self.gen_block_stmt(body);
        if is_await {
            // Extract variable name from left token for while let Some pattern
            let var_name = if let Some(super::ForInit::Variable(_, vars)) = Some(left) {
                vars.first().map(|(name, _)| {
                    syn::Ident::new(name, proc_macro2::Span::call_site())
                }).unwrap_or_else(|| syn::Ident::new("__item", proc_macro2::Span::call_site()))
            } else {
                syn::Ident::new("__item", proc_macro2::Span::call_site())
            };
            quote! {
                while let Some(#var_name) = #right_token.next().await {
                    #body_token
                }
            }
        } else {
            quote! {
                for #left_token in #right_token {
                    #body_token
                }
            }
        }
    }

    fn gen_for_init(&self, init: &Option<super::ForInit>) -> TokenStream {
        match init {
            Some(super::ForInit::Variable(kind, vars)) => {
                let decls: Vec<TokenStream> = vars
                    .iter()
                    .map(|(name, init)| {
                        let id = syn::Ident::new(name, proc_macro2::Span::call_site());
                        match init {
                            Some(expr) => {
                                let e = self.gen_expr(expr);
                                quote! { #id = #e }
                            }
                            None => quote! { #id },
                        }
                    })
                    .collect();
                let keyword = match kind {
                    super::VariableKind::Var => quote! { let mut },
                    super::VariableKind::Let => quote! { let },
                    super::VariableKind::Const => quote! { let },
                };
                quote! { #keyword #(#decls),* }
            }
            Some(super::ForInit::Expr(e)) => {
                self.gen_expr(e)
            }
            None => quote! {},
        }
    }

    fn gen_var_decl(&self, var: &VariableDecl) -> Option<TokenStream> {
        let init = var.init.as_ref()?;
        let init_expr = self.gen_expr(init);

        // Handle patterns
        if let Some(ref pattern) = var.pattern {
            let decls = self.gen_pat(pattern, &init_expr);
            if decls.is_empty() {
                return None;
            }
            let keyword = match var.kind {
                VariableKind::Var => quote! { let mut },
                VariableKind::Let => quote! { let },
                VariableKind::Const => quote! { let },
            };
            // Wrap in block if multiple declarations
            if decls.len() == 1 {
                let decl = &decls[0];
                Some(quote! { #keyword #decl; })
            } else {
                let stmts: Vec<TokenStream> = decls
                    .into_iter()
                    .map(|d| quote! { #keyword #d; })
                    .collect();
                Some(quote! { #(#stmts)* })
            }
        } else {
            // Simple variable without pattern
            let id = syn::Ident::new(&var.name, proc_macro2::Span::call_site());
            let keyword = match var.kind {
                VariableKind::Var => quote! { let mut },
                VariableKind::Let => quote! { let },
                VariableKind::Const => quote! { let },
            };
            Some(quote! { #keyword #id = #init_expr; })
        }
    }


    /// Generate variable declarations from a pattern
    /// Returns a list of "name = value" expressions
    fn gen_pat(&self, pat: &Pat, source: &TokenStream) -> Vec<TokenStream> {
        match pat {
            Pat::Ident { name, .. } => {
                let id = syn::Ident::new(name, proc_macro2::Span::call_site());
                vec![quote! { #id = #source }]
            }
            Pat::Object { props, rest } => {
                self.gen_object_pat(props, rest, source)
            }
            Pat::Array { elems, rest } => {
                self.gen_array_pat(elems, rest, source)
            }
            Pat::Rest { arg: _ } => {
                // Rest pattern: ...rest means clone the source
                vec![quote! { rest = #source.clone() }]
            }
            Pat::Default { arg, default } => {
                // Default value: use unwrap_or with the default expression
                let default_expr = self.gen_expr(default);
                let inner_decls = self.gen_pat(arg, source);
                inner_decls
                    .into_iter()
                    .map(|decl| {
                        quote! { #decl.unwrap_or(#default_expr) }
                    })
                    .collect()
            }
            Pat::Assign { left, right } => {
                // a = b pattern: assign right to left
                let right_expr = self.gen_expr(right);
                self.gen_pat(left, &right_expr)
            }
        }
    }

    /// Generate variable declarations from object destructuring pattern
    fn gen_object_pat(
        &self,
        props: &[ObjectPatProp],
        rest: &Option<Box<Pat>>,
        source: &TokenStream,
    ) -> Vec<TokenStream> {
        let mut decls = Vec::new();

        for prop in props {
            match prop {
                ObjectPatProp::Init { key, value } => {
                    let field = syn::Ident::new(key, proc_macro2::Span::call_site());
                    let field_access = quote! { #source.#field };
                    let inner_decls = self.gen_pat(value, &field_access);
                    decls.extend(inner_decls);
                }
                ObjectPatProp::Rest { arg } => {
                    let inner_decls = self.gen_pat(arg, &quote! { #source.clone() });
                    decls.extend(inner_decls);
                }
                ObjectPatProp::Spread { arg } => {
                    let inner_decls = self.gen_pat(arg, source);
                    decls.extend(inner_decls);
                }
                ObjectPatProp::Method { .. } => {
                    // Skip method definitions in patterns
                }
            }
        }

        // Handle rest at object level
        if let Some(rest_pat) = rest {
            let inner_decls = self.gen_pat(rest_pat, &quote! { #source.clone() });
            decls.extend(inner_decls);
        }

        decls
    }

    /// Generate variable declarations from array destructuring pattern
    fn gen_array_pat(
        &self,
        elems: &[Option<Pat>],
        rest: &Option<Box<Pat>>,
        source: &TokenStream,
    ) -> Vec<TokenStream> {
        let mut decls = Vec::new();

        for (i, elem) in elems.iter().enumerate() {
            let index = syn::Index::from(i);
            let index_access = quote! { #source.#index };

            if let Some(pat) = elem {
                let inner_decls = self.gen_pat(pat, &index_access);
                decls.extend(inner_decls);
            }
        }

        // Handle rest
        if let Some(rest_pat) = rest {
            let len = elems.len();
            let rest_access = quote! { #source[#len..].to_vec() };
            let inner_decls = self.gen_pat(rest_pat, &rest_access);
            decls.extend(inner_decls);
        }

        decls
    }

    fn gen_while(&self, test: &Expr, body: &Box<Stmt>) -> TokenStream {
        let test_token = self.gen_expr(test);
        let body_token = self.gen_block_stmt(body);
        quote! {
            while #test_token {
                #body_token
            }
        }
    }

    fn gen_do_while(&self, body: &Box<Stmt>, test: &Expr) -> TokenStream {
        let body_token = self.gen_block_stmt(body);
        let test_token = self.gen_expr(test);
        quote! {
            loop {
                #body_token
                if !(#test_token) { break; }
            }
        }
    }

    fn gen_break(&self, label: &Option<String>) -> TokenStream {
        match label {
            Some(l) => {
                let label_id = syn::Ident::new(l, proc_macro2::Span::call_site());
                quote! { break #label_id; }
            }
            None => quote! { break; },
        }
    }

    fn gen_continue(&self, label: &Option<String>) -> TokenStream {
        match label {
            Some(l) => {
                let label_id = syn::Ident::new(l, proc_macro2::Span::call_site());
                quote! { continue #label_id; }
            }
            None => quote! { continue; },
        }
    }

    fn gen_throw(&self, arg: &Expr) -> TokenStream {
        let expr = self.gen_expr(arg);
        quote! { std::panic::panic_any(#expr); }
    }

    fn gen_try(&self, block: &super::Block, handler: &Option<super::CatchClause>, finalizer: &Option<super::Block>) -> TokenStream {
        let block_stmts: Vec<TokenStream> = block.0.iter().filter_map(|s| self.gen_stmt(s)).collect();
        match (handler, finalizer) {
            (Some(h), Some(f)) => {
                let catch_param = syn::Ident::new(&h.param, proc_macro2::Span::call_site());
                let catch_body: Vec<TokenStream> = h.body.0.iter().filter_map(|s| self.gen_stmt(s)).collect();
                let finally_stmts: Vec<TokenStream> = f.0.iter().filter_map(|s| self.gen_stmt(s)).collect();
                quote! { { let __result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| { #(#block_stmts)* })); match __result { Ok(v) => v, Err(e) => { let #catch_param = e; #(#catch_body)* } }; #(#finally_stmts)* } }
            }
            (Some(h), None) => {
                let catch_param = syn::Ident::new(&h.param, proc_macro2::Span::call_site());
                let catch_body: Vec<TokenStream> = h.body.0.iter().filter_map(|s| self.gen_stmt(s)).collect();
                quote! { { let __result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| { #(#block_stmts)* })); match __result { Ok(v) => v, Err(e) => { let #catch_param = e; #(#catch_body)* } } } }
            }
            (None, Some(f)) => {
                let finally_stmts: Vec<TokenStream> = f.0.iter().filter_map(|s| self.gen_stmt(s)).collect();
                quote! { { #(#block_stmts)* #(#finally_stmts)* } }
            }
            (None, None) => quote! { #(#block_stmts)* },
        }
    }

    fn gen_block(&self, stmts: &[Stmt]) -> TokenStream {
        let inner: Vec<TokenStream> = stmts
            .iter()
            .filter_map(|s| self.gen_stmt(s))
            .collect();
        quote! {
            {
                #(#inner)*
            }
        }
    }

    fn gen_labeled(&self, label: &str, body: &Box<Stmt>) -> TokenStream {
        let body_token = self.gen_block_stmt(body);
        // Rust labels are lifetimes like 'loop:
        let lifetime = syn::Lifetime::new(&format!("'{}", label), proc_macro2::Span::call_site());
        quote! {
            #lifetime: #body_token
        }
    }

    fn gen_with(&self, obj: &Expr, body: &Box<Stmt>) -> TokenStream {
        let obj_token = self.gen_expr(obj);
        let body_token = self.gen_block_stmt(body);
        quote! {
            {
                let __with_obj = #obj_token;
                #body_token
            }
        }
    }

    fn gen_block_stmt(&self, stmt: &Stmt) -> TokenStream {
        match stmt {
            Stmt::Block { stmts } => {
                let inner: Vec<_> = stmts.iter()
                    .filter_map(|s| self.gen_stmt(s))
                    .collect();
                quote! { #(#inner)* }
            }
            _ => self.gen_stmt(stmt).unwrap_or_default(),
        }
    }
    
    pub(crate) fn gen_expr(&self, expr: &Expr) -> TokenStream {
        use super::Expr as E;
        match expr {
            E::String(s) => self.gen_string_expr(s),
            E::Number(n) => self.gen_number_expr(n),
            E::BigInt(n) => quote! { Value::BigInt(#n) },
            E::Boolean(b) => self.gen_bool_expr(*b),
            E::Null | E::Undefined => quote! { Value::Null },
            E::RegExp { pattern, flags } => quote! { Value::RegExp(#pattern, #flags) },
            E::Super => quote! { super },
            E::This => quote! { self },
            E::Block(stmts) => self.gen_block_expr(stmts),
            E::Invalid => panic!("codegen for Invalid expression"),
            _ => self.gen_expr_helper(expr),
        }
    }

    fn gen_expr_helper(&self, expr: &Expr) -> TokenStream {
        use super::Expr as E;
        match expr {
            E::Template { parts, exprs } => self.gen_template_expr(parts, exprs),
            E::Ident { name } => self.gen_ident_expr(name),
            E::JSX(jsx) => self.gen_jsx_expr(jsx),
            E::Bin { op, left, right } => self.gen_bin_expr(op, left, right),
            E::Unary { op, arg, prefix } => self.gen_unary_expr(op, arg, *prefix),
            E::Update { op, arg, prefix } => self.gen_update_expr(op, arg, *prefix),
            E::Logical { op, left, right } => self.gen_logical_expr(op, left, right),
            E::Cond { test, consequent, alternate } => self.gen_cond_expr(test, consequent, alternate),
            E::Assign { op, left, right } => self.gen_assign_expr(op, left, right),
            E::Array { elems } | E::Object { members: _ } => self.gen_array_expr(elems),
            E::Function(func) => self.gen_fn_expr(func),
            E::ArrowFunction { params, body, is_async } => self.gen_arrow_expr(params, body, *is_async),
            E::Await { arg } | E::Yield { arg, delegate: _ } => self.gen_await_expr(arg),
            E::Call { callee, arguments } | E::New { callee, arguments } => self.gen_call_expr(callee, arguments),
            E::Member { obj, property, computed } => self.gen_member_expr_full(obj, property, *computed),
            E::StaticMember { obj, property } => self.gen_static_member_expr(obj, property),
            E::Seq { left, right } | E::Spread { arg: _ } => self.gen_seq_expr(left, right),
            E::TypeAnnot { type_ } => {
                let _ = type_;
                quote! { Value::Null }
            }
            _ => quote! { Value::Null },
        }
    }
    
    fn gen_string_expr(&self, s: &str) -> TokenStream {
        // String literals in Rust are &str, so we can use them directly
        quote! { #s }
    }

    fn gen_number_expr(&self, n: &f64) -> TokenStream {
        quote! { #n }
    }

    fn gen_bool_expr(&self, b: bool) -> TokenStream {
        quote! { #b }
    }

    fn gen_template_expr(&self, parts: &[super::TemplatePart], exprs: &[Expr]) -> TokenStream {
        let mut result = TokenStream::new();
        let mut expr_idx = 0;
        
        for part in parts {
            match part {
                super::TemplatePart::String { value: s } => {
                    let s = s.to_string();
                    result.extend(quote! { #s.to_string() });
                }
                super::TemplatePart::Type { value: _ } => {
                    // This shouldn't happen in expression context, but handle it
                    result.extend(quote! { String::new() });
                }
            }
            
            if expr_idx < exprs.len() {
                let expr = &exprs[expr_idx];
                let expr_ts = self.gen_expr(expr);
                result.extend(quote! { + &#expr_ts.to_string() });
                expr_idx += 1;
            }
        }
        
        if result.is_empty() {
            quote! { String::new() }
        } else {
            result
        }
    }

    fn gen_ident_expr(&self, name: &str) -> TokenStream {
        let id = syn::Ident::new(name, proc_macro2::Span::call_site());
        quote! { #id }
    }

    fn gen_static_member_expr(&self, obj: &Expr, property: &str) -> TokenStream {
        if let Expr::Ident { name: obj_name } = obj {
            let obj_name_str: &str = obj_name.as_str();
            match obj_name_str {
                "Number" => {
                    return match property {
                        "NaN" => quote! { f64::NAN },
                        "POSITIVE_INFINITY" | "Infinity" => quote! { f64::INFINITY },
                        "NEGATIVE_INFINITY" => quote! { f64::NEG_INFINITY },
                        "MAX_VALUE" => quote! { f64::MAX },
                        "MIN_VALUE" => quote! { f64::MIN_POSITIVE },
                        _ => { let prop = syn::Ident::new(property, proc_macro2::Span::call_site()); quote! { #obj_name.#prop } }
                    };
                }
                "Math" => {
                    return match property {
                        "PI" => quote! { std::f64::consts::PI },
                        "E" => quote! { std::f64::consts::E },
                        "SQRT2" => quote! { std::f64::consts::SQRT2 },
                        "SQRT1_2" => quote! { std::f64::consts::FRAC_1_SQRT_2 },
                        "LN2" => quote! { std::f64::consts::LN_2 },
                        "LN10" => quote! { std::f64::consts::LN_10 },
                        "LOG2E" => quote! { std::f64::consts::LOG2_E },
                        "LOG10E" => quote! { std::f64::consts::LOG10_E },
                        _ => { let prop = syn::Ident::new(property, proc_macro2::Span::call_site()); quote! { #obj_name.#prop } }
                    };
                }
                _ => {}
            }
        }
        let obj = self.gen_expr(obj);
        let prop = syn::Ident::new(property, proc_macro2::Span::call_site());
        quote! { #obj.#prop }
    }

    fn gen_member_expr_full(&self, obj: &Expr, property: &Expr, computed: bool) -> TokenStream {
        if let Expr::Ident { name: obj_name } = obj {
            if obj_name == "Number" {
                if let Expr::Ident { name: prop_name } = property {
                    return match prop_name.as_str() {
                        "NaN" => quote! { f64::NAN },
                        "POSITIVE_INFINITY" | "Infinity" => quote! { f64::INFINITY },
                        "NEGATIVE_INFINITY" => quote! { f64::NEG_INFINITY },
                        "MAX_VALUE" => quote! { f64::MAX },
                        "MIN_VALUE" => quote! { f64::MIN_POSITIVE },
                        _ => { let prop = self.gen_expr(property); quote! { #obj_name.#prop } }
                    };
                }
            }
            if obj_name == "Math" {
                if let Expr::Ident { name: prop_name } = property {
                    return match prop_name.as_str() {
                        "PI" => quote! { std::f64::consts::PI },
                        "E" => quote! { std::f64::consts::E },
                        "SQRT2" => quote! { std::f64::consts::SQRT2 },
                        "SQRT1_2" => quote! { std::f64::consts::FRAC_1_SQRT_2 },
                        "LN2" => quote! { std::f64::consts::LN_2 },
                        "LN10" => quote! { std::f64::consts::LN_10 },
                        "LOG2E" => quote! { std::f64::consts::LOG2_E },
                        "LOG10E" => quote! { std::f64::consts::LOG10_E },
                        _ => { let prop = self.gen_expr(property); quote! { #obj_name.#prop } }
                    };
                }
            }
        }
        let obj = self.gen_expr(obj);
        let prop = self.gen_expr(property);
        if computed { quote! { #obj[#prop] } } else { quote! { #obj.#prop } }
    }

    fn gen_unary_expr(&self, op: &super::UnaryOp, arg: &Expr, _prefix: bool) -> TokenStream {
        use super::UnaryOp as U;
        let arg_ts = self.gen_expr(arg);
        match op {
            U::Plus => quote! { #arg_ts },
            U::Minus => quote! { -#arg_ts },
            U::Not | U::BitNot => quote! { !#arg_ts },
            U::Typeof => quote! { { match &#arg_ts { Value::String(_) => "string", Value::Number(_) => "number", Value::Boolean(_) => "boolean", Value::Null => "object", Value::Undefined => "undefined", Value::BigInt(_) => "bigint", Value::Function(_) => "function", Value::Object(_) | Value::Array(_) | Value::RegExp(_, _) => "object", _ => "unknown" } } },
            U::Void => quote! { () },
            U::Delete => quote! { false },
        }
    }

    fn gen_logical_expr(&self, op: &super::LogicalOp, left: &Expr, right: &Expr) -> TokenStream {
        let lhs = self.gen_expr(left);
        let rhs = self.gen_expr(right);
        let op_str = match op {
            super::LogicalOp::And => "&&",
            super::LogicalOp::Or => "||",
            super::LogicalOp::NullishCoalescing => "??",
        };
        quote! { #lhs #op_str #rhs }
    }

    fn gen_cond_expr(&self, test: &Expr, consequent: &Expr, alternate: &Expr) -> TokenStream {
        let test = self.gen_expr(test);
        let cons = self.gen_expr(consequent);
        let alt = self.gen_expr(alternate);
        quote! { if #test { #cons } else { #alt } }
    }

    fn gen_array_expr(&self, elems: &[Option<Expr>]) -> TokenStream {
        let items: Vec<_> = elems.iter()
            .map(|e| e.as_ref().map(|e| self.gen_expr(e)).unwrap_or_else(|| quote! { Value::Null }))
            .collect();
        quote! { vec![#(#items),*] }
    }

    fn gen_object_expr(&self, members: &[super::ObjectMemberExpr]) -> TokenStream {
        let entries: Vec<TokenStream> = members.iter().filter_map(|m| {
            match &m.prop {
                super::ObjectProp::Init { key, value, computed: _ } => {
                    let key_ts = match key {
                        super::PropKey::Str(s) => {
                            let s = s.to_string();
                            quote! { #s.to_string() }
                        }
                        super::PropKey::Num(n) => {
                            let n_s = n.to_string();
                            quote! { #n_s.to_string() }
                        }
                        super::PropKey::Computed { expr } => {
                            let expr_ts = self.gen_expr(expr);
                            quote! { format!("{}", #expr_ts) }
                        }
                    };
                    let value_ts = self.gen_expr(value);
                    Some(quote! { (#key_ts, #value_ts) })
                }
                _ => None, // Get, Set, Method, Spread not yet supported
            }
        }).collect();
        
        if entries.is_empty() {
            quote! { std::collections::HashMap::new() }
        } else {
            quote! { std::collections::HashMap::from([#(#entries),*]) }
        }
    }

    fn gen_fn_expr(&self, func: &super::FunctionDecl) -> TokenStream {
        self.gen_fn(func)
    }

    fn gen_arrow_expr(&self, params: &[super::Param], body: &Expr, is_async: bool) -> TokenStream {
        let params_ts = self.gen_params(params);
        let body_ts = self.gen_expr(body);
        if is_async {
            quote! { async move |#params_ts| -> Value { #body_ts } }
        } else {
            quote! { move |#params_ts| -> Value { #body_ts } }
        }
    }

    fn gen_await_expr(&self, arg: &Expr) -> TokenStream {
        let arg_ts = self.gen_expr(arg);
        quote! { #arg_ts.await }
    }

    fn gen_yield_expr(&self, arg: &Option<Box<Expr>>, delegate: bool) -> TokenStream {
        if delegate {
            if let Some(a) = arg {
                let a_ts = self.gen_expr(a);
                quote! { yield* #a_ts }
            } else {
                quote! { Value::Null }
            }
        } else {
            if let Some(a) = arg {
                let a_ts = self.gen_expr(a);
                quote! { yield #a_ts }
            } else {
                quote! { Value::Null }
            }
        }
    }

    fn gen_new_expr(&self, callee: &Expr, arguments: &[Expr]) -> TokenStream {
        // Special handling for new Promise() - generate tokio spawn
        if let Expr::Ident { name } = callee {
            if name == "Promise" {
                // new Promise((resolve, reject) => { ... }) -> tokio::spawn with channel
                return quote! { {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    tokio::spawn(async move {
                        if let Err(_) = rx.await {
                            // Promise was dropped without resolution
                        }
                    });
                    tx
                } };
            }
        }

        let callee_ts = self.gen_expr(callee);
        let args: Vec<_> = arguments.iter().map(|a| self.gen_expr(a)).collect();
        quote! { #callee_ts(#(#args),*) }
    }

    fn gen_seq_expr(&self, left: &Expr, right: &Expr) -> TokenStream {
        let lhs = self.gen_expr(left);
        let rhs = self.gen_expr(right);
        quote! { { #lhs; #rhs } }
    }

    fn gen_spread_expr(&self, arg: &Expr) -> TokenStream {
        let arg_ts = self.gen_expr(arg);
        quote! { {#arg_ts} }
    }

    /// Generate a class as a Rust struct + impl block
    pub fn gen_class(&self, class: &super::ClassDecl) -> TokenStream {
        let name = syn::Ident::new(&class.name, proc_macro2::Span::call_site());
        if class.extends.is_some() { panic!("class inheritance (extends) is not supported"); }
        let field_defs: Vec<TokenStream> = class.members.iter().filter(|m| !m.is_static).map(|m| { let field_name = syn::Ident::new(&m.name, proc_macro2::Span::call_site()); let ty = m.type_.as_ref().map(|t| self.gen_type(t)).unwrap_or_else(|| quote! { f64 }); quote! { pub #field_name: #ty } }).collect();
        let field_names: Vec<_> = class.members.iter().filter(|m| !m.is_static).map(|m| syn::Ident::new(&m.name, proc_macro2::Span::call_site())).collect();
        let mut constructor_tokens: Option<TokenStream> = None;
        let mut method_tokens: Vec<TokenStream> = Vec::new();
        for method in &class.methods {
            if method.kind == super::MethodKind::Constructor {
                let params: Vec<_> = method.params.iter().map(|p| { let pname = syn::Ident::new(&p.name, proc_macro2::Span::call_site()); let ty = p.type_.as_ref().map(|t| self.gen_type(t)).unwrap_or_else(|| quote! { f64 }); quote! { #pname: #ty } }).collect();
                constructor_tokens = Some(quote! { pub fn new(#(#params),*) -> Self { #name { #(#field_names),* } } });
            } else {
                let method_name = syn::Ident::new(&method.name, proc_macro2::Span::call_site());
                let params_with_self: Vec<TokenStream> = { let mut p = vec![quote! { &self }]; p.extend(method.params.iter().map(|pm| { let pname = syn::Ident::new(&pm.name, proc_macro2::Span::call_site()); let ty = pm.type_.as_ref().map(|t| self.gen_type(t)).unwrap_or_else(|| quote! { f64 }); quote! { #pname: #ty } })); p };
                method_tokens.push(quote! { pub fn #method_name(#(#params_with_self),*) -> f64 { #(#method_tokens)* } });
            }
        }
        let struct_body: TokenStream = if field_defs.is_empty() { quote! {} } else { let mut combined = quote! {}; for (i, field) in field_defs.iter().enumerate() { combined = if i > 0 { quote! { #combined, #field } } else { quote! { #field } }; } quote! { #combined } };
        quote! { struct #name { #struct_body } impl #name { #constructor_tokens #(#method_tokens)* } }
    }

    fn gen_block_expr(&self, stmts: &[super::Stmt]) -> TokenStream {
        let inner: Vec<_> = stmts.iter()
            .filter_map(|s| self.gen_stmt(s))
            .collect();
        quote! { { #(#inner)* } }
    }

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
        use super::JSXAttrValue;
        let mut props_fields: Vec<TokenStream> = Vec::new();
        for attr in attrs {
            match attr {
                super::JSXAttr::Attr { name: prop_name, value } => {
                    let key = syn::Ident::new(prop_name, proc_macro2::Span::call_site());
                    match value {
                        Some(JSXAttrValue::String(s)) => props_fields.push(quote! { #key: #s.to_string() }),
                        Some(JSXAttrValue::Expr(expr)) => {
                            let val = self.gen_expr(expr);
                            props_fields.push(quote! { #key: #val });
                        }
                        Some(JSXAttrValue::Empty) | None => props_fields.push(quote! { #key: true }),
                    }
                }
                super::JSXAttr::Spread { expr } => {
                    let expr_tokens = self.gen_expr(expr);
                    props_fields.push(quote! { /* spread: #expr_tokens */ });
                }
            }
        }
        let child_nodes: Vec<TokenStream> = self.gen_jsx_children(children);
        // For member expressions like "React.Foo", we can't use syn::Ident
        // Instead, use the string directly in the quote
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
            if name == "fetch" { let url = arguments.first().map(|a| self.gen_expr(a)).unwrap_or_else(|| quote! { String::new() }); return quote! { reqwest::get(#url).await? }; }
            if name == "setTimeout" { let duration = arguments.get(1).map(|a| self.gen_expr(a)).unwrap_or_else(|| quote! { 0 }); return quote! { tokio::time::sleep(std::time::Duration::from_millis(#duration as u64)).await }; }
            if name == "setInterval" { let duration = arguments.get(1).map(|a| self.gen_expr(a)).unwrap_or_else(|| quote! { 0 }); return quote! { tokio::time::interval(std::time::Duration::from_millis(#duration as u64)) }; }
        }
        if let Expr::StaticMember { obj, property } = callee {
            if let Expr::Ident { name } = obj.as_ref() {
                if name == "console" {
                    let is_error = property == "error" || property == "warn";
                    if property == "log" || property == "error" || property == "info" || property == "table" || property == "warn" || property == "assert" {
                        let args: Vec<_> = arguments.iter().map(|a| self.gen_expr(a)).collect();
                        if property == "assert" {
                            return if arguments.len() >= 2 { let cond = args.first().unwrap(); let msg = args.get(1).unwrap(); quote! { assert!(#cond, "{}", #msg) } } else if arguments.len() == 1 { quote! { assert!(#(args.first().unwrap())) } } else { quote! { () } };
                        }
                        if arguments.len() == 1 { return if is_error { syn::parse_quote! { eprintln!("{}", #(#args),*) } } else { syn::parse_quote! { println!("{}", #(#args),*) } }; }
                        let format_args: Vec<_> = arguments.iter().map(|_| quote! { "{}" }).collect();
                        return if is_error { syn::parse_quote! { eprintln!(#(#format_args),*, #(#args),*) } } else { syn::parse_quote! { println!(#(#format_args),*, #(#args),*) } };
                    }
                }
            }
        }
        let callee = self.gen_expr(callee);
        let args: Vec<_> = arguments.iter().map(|a| { let arg = self.gen_expr(a); if matches!(a, Expr::String(_)) { quote! { #arg.to_string() } } else { arg } }).collect();
        quote! { #callee(#(#args),*) }
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
            B::Add => quote! { + },
            B::Sub => quote! { - },
            B::Mul => quote! { * },
            B::Div => quote! { / },
            B::Mod => quote! { % },
            B::Exp => quote! { powf() },
            B::DivStrict => quote! { / },
            B::BitXor => quote! { ^ },
            B::BitAnd => quote! { & },
            B::BitOr => quote! { | },
            B::Shl => quote! { << },
            B::Shr => quote! { >> },
            B::UShr => quote! { >>> },
            B::Eq => quote! { == },
            B::StrictEq => quote! { === },
            B::Neq => quote! { != },
            B::StrictNeq => quote! { !== },
            B::Lt => quote! { < },
            B::Lte => quote! { <= },
            B::Gt => quote! { > },
            B::Gte => quote! { >= },
            B::Instanceof => quote! { instanceof },
            B::In => quote! { in },
            B::LogicalAnd => quote! { && },
            B::LogicalOr => quote! { || },
            B::NullishCoalescing => quote! { ?? },
        }
    }
}

impl Default for QuoteCodegen {
    fn default() -> Self {
        Self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transpile::hir::*;

    #[test]
    fn test_gen_fn() {
        let cg = QuoteCodegen::default();
        let func = FunctionDecl {
            name: "greet".into(),
            generics: vec![],
            params: vec![Param {
                name: "name".into(),
                type_: Some(Type::String),
                default: None,
                optional: false,
                pattern: None,
                ownership: Ownership::Borrow,
            }],
            return_type: Some(Type::String),
            body: Some(Block(vec![Stmt::Return {
                arg: Some(Expr::Ident { name: "name".into() }),
            }])),
            is_async: false,
            is_generator: false,
            decorators: vec![],
            throws: false,
            error_type: None,
        };

        let tokens = cg.gen_fn(&func);
        let s = tokens.to_string();
        assert!(s.contains("fn greet"));
        assert!(s.contains("& String") || s.contains("&String"));
        assert!(s.contains("-> String"));
    }

    #[test]
    fn test_gen_for_loop() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::For {
            init: Some(ForInit::Variable(VariableKind::Let, vec![("i".to_string(), Some(Expr::Number(0.0)))])),
            test: Some(Expr::Bin {
                op: BinaryOp::Lt,
                left: Box::new(Expr::Ident { name: "i".into() }),
                right: Box::new(Expr::Number(10.0)),
            }),
            update: Some(Expr::Update {
                op: UpdateOp::PlusPlus,
                arg: Box::new(Expr::Ident { name: "i".into() }),
                prefix: true,
            }),
            body: Box::new(Stmt::Block { stmts: vec![] }),
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("for"));
        assert!(s.contains("let i = 0"));
        assert!(s.contains("i < 10"));
    }

    #[test]
    fn test_gen_while_loop() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::While {
            test: Expr::Boolean(true),
            body: Box::new(Stmt::Block { stmts: vec![] }),
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("while true"));
    }

    #[test]
    fn test_gen_do_while_loop() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::DoWhile {
            body: Box::new(Stmt::Block { stmts: vec![] }),
            test: Expr::Boolean(true),
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("loop"));
    }

    #[test]
    fn test_gen_switch() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::Switch {
            discriminant: Expr::Ident { name: "x".into() },
            cases: vec![
                SwitchCase {
                    test: Some(Expr::Number(1.0)),
                    consequent: vec![Stmt::Return { arg: Some(Expr::String("one".into())) }],
                },
                SwitchCase {
                    test: Some(Expr::Number(2.0)),
                    consequent: vec![Stmt::Return { arg: Some(Expr::String("two".into())) }],
                },
                SwitchCase {
                    test: None,
                    consequent: vec![Stmt::Return { arg: Some(Expr::String("other".into())) }],
                },
            ],
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("match x"));
    }

    #[test]
    fn test_gen_try_catch() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::Try {
            block: Block(vec![Stmt::Return { arg: Some(Expr::Number(1.0)) }]),
            handler: Some(CatchClause {
                param: "e".into(),
                body: Box::new(Block(vec![Stmt::Return { arg: Some(Expr::Number(0.0)) }])),
            }),
            finalizer: None,
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("catch_unwind"));
    }

    #[test]
    fn test_gen_break_continue() {
        let cg = QuoteCodegen::default();
        let break_stmt = Stmt::Break { label: None };
        let continue_stmt = Stmt::Continue { label: None };

        let break_tokens = cg.gen_stmt(&break_stmt);
        assert!(break_tokens.is_some());
        assert!(break_tokens.unwrap().to_string().contains("break"));

        let continue_tokens = cg.gen_stmt(&continue_stmt);
        assert!(continue_tokens.is_some());
        assert!(continue_tokens.unwrap().to_string().contains("continue"));
    }

    #[test]
    fn test_gen_throw() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::Throw {
            arg: Expr::String("error".into()),
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("panic_any"));
    }

    #[test]
    fn test_gen_return() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::Return { arg: Some(Expr::Number(42.0)) };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("return"));
    }

    #[test]
    fn test_gen_labeled() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::Labeled {
            label: "loop".into(),
            body: Box::new(Stmt::While {
                test: Expr::Boolean(true),
                body: Box::new(Stmt::Break { label: None }),
            }),
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        // Rust labels are lifetimes like 'loop : (with spaces around punctuation in token stream)
        assert!(s.contains("'loop"), "Expected string to contain 'loop, got: {:?}", s);
    }

    #[test]
    fn test_gen_union_type() {
        let cg = QuoteCodegen::default();
        let union_type = Type::Union {
            types: vec![Type::String, Type::Number],
        };

        let tokens = cg.gen_type(&union_type);
        let s = tokens.to_string();
        assert!(s.contains("enum"));
        assert!(s.contains("Variant"));
    }

    #[test]
    fn test_gen_intersection_type() {
        let cg = QuoteCodegen::default();
        let intersection_type = Type::Intersection {
            types: vec![
                Type::Object { members: vec![TypeMember { key: "a".into(), type_: Type::String, optional: false, readonly: false }] },
                Type::Object { members: vec![TypeMember { key: "b".into(), type_: Type::Number, optional: false, readonly: false }] },
            ],
        };

        let tokens = cg.gen_type(&intersection_type);
        let s = tokens.to_string();
        assert!(s.contains("struct"));
    }

    #[test]
    fn test_gen_literal_type() {
        let cg = QuoteCodegen::default();
        let literal_type = Type::Literal {
            kind: LiteralKind::String,
            value: "hello".into(),
        };

        let tokens = cg.gen_type(&literal_type);
        let s = tokens.to_string();
        assert!(s.contains("hello"));
    }

    #[test]
    fn test_gen_console_log() {
        let cg = QuoteCodegen::default();
        let expr = Expr::Call {
            callee: Box::new(Expr::StaticMember {
                obj: Box::new(Expr::Ident { name: "console".into() }),
                property: "log".into(),
            }),
            arguments: vec![Expr::String("hello".into())],
        };
        let tokens = cg.gen_expr(&expr);
        let s = tokens.to_string();
        // proc_macro may add spaces: "println ! (...)" is still println!
        assert!(s.contains("println") && s.contains("hello"), "console.log should generate println with hello, got: {}", s);
    }

    #[test]
    fn test_gen_string_concat() {
        let cg = QuoteCodegen::default();
        let expr = Expr::Bin {
            op: BinaryOp::Add,
            left: Box::new(Expr::String("Hello, ".into())),
            right: Box::new(Expr::Ident { name: "name".into() }),
        };
        let tokens = cg.gen_expr(&expr);
        let s = tokens.to_string();
        // quote may add spaces: "format ! (...)" is still format!
        assert!(s.contains("format"), "string concat should use format!, got: {}", s);
    }

    #[test]
    fn test_gen_import_named() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::ImportNamed {
            source: "react".into(),
            specifiers: vec![
                ImportSpecifier::Named { name: "useState".into(), alias: None },
                ImportSpecifier::Named { name: "Component".into(), alias: Some("Comp".into()) },
            ],
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("use"), "import should produce use statement");
        assert!(s.contains("react"), "import should contain module name");
        assert!(s.contains("useState"), "import should contain imported name");
        assert!(s.contains("as"), "import with alias should contain 'as'");
    }

    #[test]
    fn test_gen_import_default() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::ImportDefault {
            source: "react".into(),
            local: "React".into(),
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("use"), "import should produce use statement");
        assert!(s.contains("react"), "import should contain module name");
        assert!(s.contains("React"), "import should contain local name");
    }

    #[test]
    fn test_gen_import_namespace() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::ImportNamed {
            source: "lodash".into(),
            specifiers: vec![
                ImportSpecifier::Namespace { name: "_".into() },
            ],
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("use"), "import should produce use statement");
        assert!(s.contains("lodash"), "import should contain module name");
        assert!(s.contains("*"), "namespace import should contain *");
    }

    #[test]
    fn test_gen_export_named() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::ExportNamed {
            specifiers: vec![
                Export::Named { name: "x".into() },
            ],
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("pub"), "export named should produce pub");
        assert!(s.contains("x"), "export named should contain name");
    }

    #[test]
    fn test_gen_export_reexport() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::ExportNamed {
            specifiers: vec![
                Export::ReExport { source: "mod".into(), names: vec!["a".into(), "b".into()] },
            ],
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("pub use"), "re-export should produce pub use");
        assert!(s.contains("mod"), "re-export should contain module name");
        assert!(s.contains("a"), "re-export should contain names");
    }

    #[test]
    fn test_gen_export_all() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::ExportNamed {
            specifiers: vec![
                Export::All { source: "mod".into() },
            ],
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("pub use"), "export all should produce pub use");
        assert!(s.contains("mod"), "export all should contain module name");
        assert!(s.contains("*"), "export all should contain *");
    }

    #[test]
    fn test_gen_export_default_expr() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::ExportDefault {
            expr: Expr::Number(42.0),
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("pub const"), "export default number should produce pub const");
        assert!(s.contains("default"), "export default should contain 'default'");
    }

    #[test]
    fn test_gen_export_default_function() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::ExportDefault {
            expr: Expr::Function(FunctionDecl {
                name: "myFunc".into(),
                generics: vec![],
                params: vec![],
                return_type: None,
                body: Some(Block(vec![])),
                is_async: false,
                is_generator: false,
                decorators: vec![],
                throws: false,
                error_type: None,
            }),
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("fn myFunc"), "export default function should produce fn with name");
    }

    #[test]
    fn test_gen_export_default_arrow() {
        let cg = QuoteCodegen::default();
        let stmt = Stmt::ExportDefault {
            expr: Expr::ArrowFunction {
                params: vec![Param {
                    name: "x".into(),
                    type_: Some(Type::Number),
                    default: None,
                    optional: false,
                    pattern: None,
                    ownership: Ownership::Owned,
                }],
                body: Box::new(Expr::Number(42.0)),
                is_async: false,
            },
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        assert!(s.contains("fn default"), "export default arrow should produce fn default");
    }

    #[test]
    fn test_gen_module_path_simplification() {
        let cg = QuoteCodegen::default();
        // Test with a path like "./utils/helper"
        let stmt = Stmt::ImportNamed {
            source: "./utils/helper".into(),
            specifiers: vec![
                ImportSpecifier::Named { name: "foo".into(), alias: None },
            ],
        };

        let tokens = cg.gen_stmt(&stmt);
        assert!(tokens.is_some());
        let s = tokens.unwrap().to_string();
        // Should use last path component "helper", not full path
        assert!(s.contains("helper"), "module path should be simplified to last component");
        assert!(!s.contains("./utils/helper"), "full path should not appear");
    }
}
