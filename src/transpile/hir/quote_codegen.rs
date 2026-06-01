//! TokenStream-based Rust codegen using quote
//!
//! Generates proper Rust TokenStream instead of strings.
//! This enables compile-time validation and better error messages.
//!
//! allow:complexity,too_many_lines

use proc_macro2::TokenStream;
use quote::quote;

use super::{Expr, FunctionDecl, Ownership, Stmt, Type};

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
        let body = self.gen_body(&func.body);
        
        let vis = quote! { pub };
        let async_kw = if func.is_async { quote! { async } } else { quote! {} };
        
        quote! {
            #vis #async_kw fn #name(#params) #ret_type {
                #body
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
        let name = syn::Ident::new(&param.name, proc_macro2::Span::call_site());
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
    
    // allow:complexity,too_many_lines
    fn gen_type(&self, ty: &Type) -> TokenStream {
        use super::Type as T;
        match ty {
            T::String | T::Number | T::Boolean => self.gen_prim_type(ty),
            T::Void | T::Never | T::Undefined | T::Null | T::Unknown | T::Any | T::BigInt => self.gen_meta_type(ty),
            T::Array { elem } => self.gen_array_type(elem),
            T::Ref { name, generics } => self.gen_ref_type(name, generics),
            T::Object { members } => self.gen_object_type(members),
            T::Union { types } => self.gen_union_type(types),
            T::Intersection { types } => self.gen_intersection_type(types),
            T::Literal { kind, value } => self.gen_literal_type(kind, value),
            T::Template { parts, values } => self.gen_template_type(parts, values),
            T::Function { params, ret } => self.gen_fn_type(params, ret),
            T::Index { obj, index } => {
                let obj_t = self.gen_type(obj);
                let index_t = self.gen_type(index);
                quote! { std::collections::HashMap<#obj_t, #index_t> }
            }
            T::Mapped { from, to } => {
                let from_t = self.gen_type(from);
                let to_t = self.gen_type(to);
                quote! { std::collections::HashMap<#from_t, #to_t> }
            }
            T::Conditional { check, extends, true_type, false_type } => {
                let check_t = self.gen_type(check);
                let extends_t = self.gen_type(extends);
                let true_t = self.gen_type(true_type);
                let false_t = self.gen_type(false_type);
                quote! { if #check_t: #extends_t { #true_t } else { #false_t } }
            }
            T::This => quote! { Self },
            T::Symbol => quote! { std::sync::Arc<std::fmt::Debug> },
            T::Query { expr } => {
                let _ = expr;
                quote! { Value }
            }
            T::Infer { name } => {
                let _ = name;
                quote! { Value }
            }
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
                super::TemplatePart::String(s) => {
                    let s = s.to_string();
                    tokens.extend(quote! { #s });
                }
                super::TemplatePart::Type(t) => {
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
    
    // allow:complexity,too_many_lines
    fn gen_stmt(&self, stmt: &Stmt) -> Option<TokenStream> {
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
            S::Block(stmts) => Some(self.gen_block(stmts)),
            S::Labeled { label, body } => Some(self.gen_labeled(label, body)),
            S::With { obj, body } => Some(self.gen_with(obj, body)),
            S::FunctionDecl(func) => Some(self.gen_fn(func)),
            S::Class(_) => None, // Class codegen not yet implemented
            S::ExportNamed { .. } | S::ExportDefault { .. } => None, // Export handled elsewhere
            S::ImportNamed { .. } | S::ImportDefault { .. } => None, // Import handled elsewhere
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
        let left_token = self.gen_for_init(&Some(left));
        let right_token = self.gen_expr(right);
        let body_token = self.gen_block_stmt(body);
        quote! {
            for #left_token in #right_token {
                #body_token
            }
        }
    }

    fn gen_for_of(&self, left: &super::ForInit, right: &Expr, body: &Box<Stmt>, is_await: bool) -> TokenStream {
        let left_token = self.gen_for_init(&Some(left));
        let right_token = self.gen_expr(right);
        let body_token = self.gen_block_stmt(body);
        if is_await {
            quote! {
                for await #left_token in #right_token {
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

    // allow:complexity,too_many_lines
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
                    super::VariableKind::Var => quote! { var },
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

    // allow:complexity,too_many_lines
    fn gen_try(&self, block: &super::Block, handler: &Option<super::CatchClause>, finalizer: &Option<super::Block>) -> TokenStream {
        let block_stmts: Vec<TokenStream> = block.0.iter()
            .filter_map(|s| self.gen_stmt(s))
            .collect();
        match (handler, finalizer) {
            (Some(h), Some(f)) => {
                let catch_param = syn::Ident::new(&h.param, proc_macro2::Span::call_site());
                let catch_body: Vec<TokenStream> = h.body.0.iter()
                    .filter_map(|s| self.gen_stmt(s))
                    .collect();
                let finally_stmts: Vec<TokenStream> = f.0.iter()
                    .filter_map(|s| self.gen_stmt(s))
                    .collect();
                quote! {
                    {
                        let __result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            #(#block_stmts)*
                        }));
                        match __result {
                            Ok(v) => v,
                            Err(e) => {
                                let #catch_param = e;
                                #(#catch_body)*
                            }
                        };
                        #(#finally_stmts)*
                    }
                }
            }
            (Some(h), None) => {
                let catch_param = syn::Ident::new(&h.param, proc_macro2::Span::call_site());
                let catch_body: Vec<TokenStream> = h.body.0.iter()
                    .filter_map(|s| self.gen_stmt(s))
                    .collect();
                quote! {
                    {
                        let __result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            #(#block_stmts)*
                        }));
                        match __result {
                            Ok(v) => v,
                            Err(e) => {
                                let #catch_param = e;
                                #(#catch_body)*
                            }
                        }
                    }
                }
            }
            (None, Some(f)) => {
                let finally_stmts: Vec<TokenStream> = f.0.iter()
                    .filter_map(|s| self.gen_stmt(s))
                    .collect();
                quote! {
                    {
                        #(#block_stmts)*
                        #(#finally_stmts)*
                    }
                }
            }
            (None, None) => {
                quote! { #(#block_stmts)* }
            }
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
        let label_id = syn::Ident::new(label, proc_macro2::Span::call_site());
        let body_token = self.gen_block_stmt(body);
        quote! {
            #label_id: #body_token
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
            Stmt::Block(stmts) => {
                let inner: Vec<_> = stmts.iter()
                    .filter_map(|s| self.gen_stmt(s))
                    .collect();
                quote! { #(#inner)* }
            }
            _ => self.gen_stmt(stmt).unwrap_or_default(),
        }
    }
    
    fn gen_expr(&self, expr: &Expr) -> TokenStream {
        // Dispatch to specific handlers
        self.gen_lit_expr(expr)
            .or_else(|| self.gen_ident_expr(expr))
            .or_else(|| self.gen_bin_expr_opt(expr))
            .or_else(|| self.gen_assign_expr_opt(expr))
            .or_else(|| self.gen_update_expr_opt(expr))
            .or_else(|| self.gen_call_expr_opt(expr))
            .or_else(|| self.gen_member_expr_opt(expr))
            .unwrap_or_else(|| quote! { Value::Null })
    }
    
    fn gen_lit_expr(&self, expr: &Expr) -> Option<TokenStream> {
        use super::Expr as E;
        match expr {
            E::Number(n) => Some(quote! { #n }),
            E::String(s) => Some(quote! { #s.to_string() }),
            E::Boolean(b) => Some(quote! { #b }),
            E::Null | E::Undefined => Some(quote! { Value::Null }),
            _ => None,
        }
    }
    
    fn gen_ident_expr(&self, expr: &Expr) -> Option<TokenStream> {
        if let super::Expr::Ident { name } = expr { Some(self.gen_ident(name)) } else { None }
    }
    
    fn gen_bin_expr_opt(&self, expr: &Expr) -> Option<TokenStream> {
        if let super::Expr::Bin { op, left, right } = expr { Some(self.gen_bin_expr(op, left, right)) } else { None }
    }
    
    fn gen_assign_expr_opt(&self, expr: &Expr) -> Option<TokenStream> {
        if let super::Expr::Assign { op, left, right } = expr {
            Some(self.gen_assign_expr(op, left, right))
        } else { None }
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
    
    fn gen_update_expr_opt(&self, expr: &Expr) -> Option<TokenStream> {
        if let super::Expr::Update { op, arg, prefix } = expr {
            Some(self.gen_update_expr(op, arg, *prefix))
        } else { None }
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
    
    fn gen_call_expr_opt(&self, expr: &Expr) -> Option<TokenStream> {
        if let super::Expr::Call { callee, arguments } = expr { Some(self.gen_call_expr(callee, arguments)) } else { None }
    }
    
    fn gen_member_expr_opt(&self, expr: &Expr) -> Option<TokenStream> {
        if let super::Expr::StaticMember { obj, property } = expr { Some(self.gen_member_expr(obj, property)) } else { None }
    }
    
    fn gen_nullish(&self) -> TokenStream { quote! { Value::Null } }
    
    fn gen_number(&self, n: &f64) -> TokenStream { quote! { #n } }
    fn gen_string(&self, s: &str) -> TokenStream { quote! { #s.to_string() } }
    fn gen_bool(&self, b: bool) -> TokenStream { quote! { #b } }
    
    fn gen_ident(&self, name: &str) -> TokenStream {
        let id = syn::Ident::new(name, proc_macro2::Span::call_site());
        quote! { #id }
    }
    
    fn gen_bin_expr(&self, op: &super::BinaryOp, left: &Expr, right: &Expr) -> TokenStream {
        let lhs = self.gen_expr(left);
        let rhs = self.gen_expr(right);
        let op = self.bin_op(op);
        quote! { #lhs #op #rhs }
    }
    
    fn gen_call_expr(&self, callee: &Expr, arguments: &[Expr]) -> TokenStream {
        let callee = self.gen_expr(callee);
        let args: Vec<_> = arguments.iter().map(|a| self.gen_expr(a)).collect();
        quote! { #callee(#(#args),*) }
    }
    
    fn gen_member_expr(&self, obj: &Expr, property: &str) -> TokenStream {
        let obj = self.gen_expr(obj);
        let prop = syn::Ident::new(property, proc_macro2::Span::call_site());
        quote! { #obj.#prop }
    }
    
    fn bin_op(&self, op: &super::BinaryOp) -> TokenStream {
        self.arith_bin_op(op).or_else(|| self.cmp_bin_op(op)).unwrap_or_else(|| quote! { == })
    }
    
    fn arith_bin_op(&self, op: &super::BinaryOp) -> Option<TokenStream> {
        use super::BinaryOp as B;
        match op {
            B::Add => Some(quote! { + }),
            B::Sub => Some(quote! { - }),
            B::Mul => Some(quote! { * }),
            B::Div => Some(quote! { / }),
            _ => None,
        }
    }
    
    fn cmp_bin_op(&self, op: &super::BinaryOp) -> Option<TokenStream> {
        Self::cmp_bin_op_str(op).map(|s| quote! { #s })
    }

    fn cmp_bin_op_str(op: &super::BinaryOp) -> Option<&'static str> {
        use super::BinaryOp as B;
        match op {
            B::Eq => Some("=="),
            B::Neq => Some("!="),
            B::Lt => Some("<"),
            B::Lte => Some("<="),
            B::Gt => Some(">"),
            B::Gte => Some(">="),
            B::StrictEq => Some("==="),
            B::StrictNeq => Some("!=="),
            _ => None,
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
            body: Box::new(Stmt::Block(vec![])),
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
            body: Box::new(Stmt::Block(vec![])),
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
            body: Box::new(Stmt::Block(vec![])),
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
        assert!(s.contains("loop:"));
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
}
