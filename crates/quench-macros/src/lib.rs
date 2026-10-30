//! Generate operation entry points from the interpreter's authoritative match.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Expr, FnArg, ImplItemFn, Pat, Stmt, parse_macro_input};

/// Derive callable semantic operations and a selector from a method whose first
/// statement matches `Op`. The remaining statements are the common completion.
/// Interpreter dispatch calls those same bodies; native publication can resolve
/// an operation once without retaining the interpreter's whole opcode match.
#[proc_macro_attribute]
pub fn operation_semantics(attribute: TokenStream, input: TokenStream) -> TokenStream {
    let selector = parse_macro_input!(attribute as syn::Ident);
    let method = parse_macro_input!(input as ImplItemFn);
    derive_operations(selector, method)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn derive_operations(
    selector: syn::Ident,
    mut method: ImplItemFn,
) -> syn::Result<proc_macro2::TokenStream> {
    let Some(Stmt::Expr(Expr::Match(dispatch), _)) = method.block.stmts.first() else {
        return Err(syn::Error::new_spanned(
            &method.block,
            "semantic method must begin with an Op match",
        ));
    };
    if !method.sig.generics.params.is_empty()
        || method.sig.asyncness.is_some()
        || method.sig.unsafety.is_some()
    {
        return Err(syn::Error::new_spanned(
            &method.sig,
            "semantic method must be synchronous, safe and non-generic",
        ));
    }
    let mut arguments = Vec::new();
    let mut types = Vec::new();
    for argument in &method.sig.inputs {
        match argument {
            FnArg::Receiver(receiver)
                if receiver.reference.is_some() && receiver.mutability.is_some() =>
            {
                types.push(quote!(&mut Self));
            }
            FnArg::Typed(argument) => {
                let Pat::Ident(name) = &*argument.pat else {
                    return Err(syn::Error::new_spanned(
                        argument,
                        "semantic operands must have names",
                    ));
                };
                let name = &name.ident;
                arguments.push(quote!(#name));
                let ty = &argument.ty;
                types.push(quote!(#ty));
            }
            _ => {
                return Err(syn::Error::new_spanned(
                    argument,
                    "semantic method requires &mut self",
                ));
            }
        }
    }
    let completion = &method.block.stmts[1..];
    let scrutinee = &dispatch.expr;
    let mut helpers = Vec::new();
    let mut dispatch_arms = Vec::new();
    let mut selector_arms = Vec::new();
    for (index, arm) in dispatch.arms.iter().enumerate() {
        if arm.guard.is_some() {
            return Err(syn::Error::new_spanned(
                arm,
                "semantic dispatch guards must live inside the operation",
            ));
        }
        let name = format_ident!("{}_operation_{}", method.sig.ident, index);
        let mut signature = method.sig.clone();
        signature.ident = name.clone();
        let body = &arm.body;
        let pattern = &arm.pat;
        helpers.push(quote! {
            // Every operation has the same ABI, even when it does not use
            // every operand. Abrupt completions bypass the common epilogue.
            #[allow(unused_variables, unreachable_code)]
            #[inline(always)]
            #signature { #body; #(#completion)* }
        });
        dispatch_arms.push(quote!(#pattern => self.#name(#(#arguments),*),));
        selector_arms.push(quote!(#pattern => Self::#name,));
    }
    let visibility = &method.vis;
    let output = &method.sig.output;
    let selector_method = quote! {
        #[inline(always)]
        #visibility fn #selector(op: Op) -> fn(#(#types),*) #output {
            match op { #(#selector_arms)* }
        }
    };
    method.block = syn::parse_quote!({ match #scrutinee { #(#dispatch_arms)* } });
    Ok(quote! { #method #selector_method #(#helpers)* })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_state_dependent_dispatch_guards() {
        let method = syn::parse_quote! {
            fn step(&mut self, i: Instruction, enabled: bool) -> Result<(), Error> {
                match i.op() {
                    Op::Call if enabled => return Ok(()),
                    _ => {}
                }
                Ok(())
            }
        };
        let error = derive_operations(syn::parse_quote!(resolve), method).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("guards must live inside the operation")
        );
    }

    #[test]
    fn rejects_a_method_without_an_authoritative_operation_match() {
        let method = syn::parse_quote! {
            fn step(&mut self) -> Result<(), Error> { Ok(()) }
        };
        let error = derive_operations(syn::parse_quote!(resolve), method).unwrap_err();
        assert!(error.to_string().contains("must begin with an Op match"));
    }
}
