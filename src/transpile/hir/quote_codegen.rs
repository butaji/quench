//! TokenStream-based Rust codegen using quote
//! 
//! Generates proper Rust TokenStream instead of strings.
//! This enables compile-time validation and better error messages.

use proc_macro2::TokenStream;
use quote::{quote, ToTokens};

use super::{Expr, FunctionDecl, Ownership, Stmt, Type};

/// Quote-based code generator
pub struct QuoteCodegen;

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
    
    fn gen_type(&self, ty: &Type) -> TokenStream {
        use super::Type as T;
        match ty {
            T::String | T::Number | T::Boolean => self.gen_prim_type(ty),
            T::Void | T::Never | T::Unknown | T::Any | T::BigInt => self.gen_meta_type(ty),
            T::Array { elem } => self.gen_array_type(elem),
            T::Ref { name, generics } => self.gen_ref_type(name, generics),
            T::Object { members } => self.gen_object_type(members),
            _ => quote! { Value },
        }
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
            T::Unknown | T::Any => quote! { Value },
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
        quote! { { #(#fields),* } }
    }
    
    fn gen_stmt(&self, stmt: &Stmt) -> Option<TokenStream> {
        use super::Stmt as S;
        match stmt {
            S::Expr { expr } => Some(self.gen_expr_stmt(expr)),
            S::Return { arg } => Some(self.gen_return(arg)),
            S::If { test, consequent, alternate } => {
                let alt_stmt = alternate.as_ref().map(|b| b.as_ref());
                Some(self.gen_if(test, consequent, alt_stmt))
            }
            _ => None,
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
                let alt = self.gen_block_stmt(a);
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
            .or_else(|| self.gen_call_expr_opt(expr))
            .or_else(|| self.gen_member_expr_opt(expr))
            .unwrap_or_else(|| quote! { Value::Null })
    }
    
    fn gen_lit_expr(&self, expr: &Expr) -> Option<TokenStream> {
        use super::Expr as E;
        match expr {
            E::Number(n) => Some(self.gen_number(n)),
            E::String(s) => Some(self.gen_string(s)),
            E::Boolean(b) => Some(self.gen_bool(*b)),
            E::Null | E::Undefined => Some(self.gen_nullish()),
            _ => None,
        }
    }
    
    fn gen_ident_expr(&self, expr: &Expr) -> Option<TokenStream> {
        if let super::Expr::Ident { name } = expr { Some(self.gen_ident(name)) } else { None }
    }
    
    fn gen_bin_expr_opt(&self, expr: &Expr) -> Option<TokenStream> {
        if let super::Expr::Bin { op, left, right } = expr { Some(self.gen_bin_expr(op, left, right)) } else { None }
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
        self.arith_bin_op(op).or_else(|| self.cmp_bin_op(op)).unwrap_or_else(|| self.cmp_op("=="))
    }
    
    fn arith_bin_op(&self, op: &super::BinaryOp) -> Option<TokenStream> {
        use super::BinaryOp as B;
        match op {
            B::Add => Some(self.arith_op("+")),
            B::Sub => Some(self.arith_op("-")),
            B::Mul => Some(self.arith_op("*")),
            B::Div => Some(self.arith_op("/")),
            _ => None,
        }
    }
    
    fn cmp_bin_op(&self, op: &super::BinaryOp) -> Option<TokenStream> {
        use super::BinaryOp as B;
        match op {
            B::Eq => Some(self.cmp_op("==")),
            B::Neq => Some(self.cmp_op("!=")),
            B::Lt => Some(self.cmp_op("<")),
            B::Lte => Some(self.cmp_op("<=")),
            B::Gt => Some(self.cmp_op(">")),
            B::Gte => Some(self.cmp_op(">=")),
            _ => None,
        }
    }
    
    fn arith_op(&self, op: &str) -> TokenStream {
        let op = syn::Ident::new(op, proc_macro2::Span::call_site());
        quote! { #op }
    }
    
    fn cmp_op(&self, op: &str) -> TokenStream {
        let op = syn::Ident::new(op, proc_macro2::Span::call_site());
        quote! { #op }
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
}
