//! Generate operation entry points from the interpreter's authoritative match.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Expr, FnArg, ImplItemFn, Pat, Stmt, parse_macro_input};

struct NativeExportLayout {
    version: syn::LitInt,
    prefix: syn::LitStr,
}

impl syn::parse::Parse for NativeExportLayout {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let version = input.parse()?;
        input.parse::<syn::Token![,]>()?;
        Ok(Self {
            version,
            prefix: input.parse()?,
        })
    }
}

/// Generate host exports from the same contiguous opcode inventory as dispatch.
/// The version owns the call-context, operand and image-table ABI; change it when
/// those layouts change. Opcode inventory changes also change the marker.
#[proc_macro_attribute]
pub fn native_operation_exports(attribute: TokenStream, input: TokenStream) -> TokenStream {
    let layout = parse_macro_input!(attribute as NativeExportLayout);
    let operations = parse_macro_input!(input as syn::ItemEnum);
    derive_native_exports(layout, operations)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn derive_native_exports(
    layout: NativeExportLayout,
    operations: syn::ItemEnum,
) -> syn::Result<proc_macro2::TokenStream> {
    const FNV1A_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV1A_PRIME: u64 = 0x100_0000_01b3;
    const OPCODE_NAME_TERMINATOR: u8 = 0;
    let version = layout.version.base10_parse::<u32>()?;
    let prefix = layout.prefix;
    let mut hash = FNV1A_OFFSET_BASIS;
    let mut exports = Vec::new();
    let mut export_arms = Vec::new();
    for (index, variant) in operations.variants.iter().enumerate() {
        if !matches!(variant.fields, syn::Fields::Unit) || variant.discriminant.is_some() {
            return Err(syn::Error::new_spanned(
                variant,
                "native opcodes must be contiguous fieldless variants",
            ));
        }
        if variant.attrs.iter().any(|attribute| {
            attribute.path().is_ident("cfg") || attribute.path().is_ident("cfg_attr")
        }) {
            return Err(syn::Error::new_spanned(
                variant,
                "native opcode inventory cannot have conditional variants",
            ));
        }
        let name = &variant.ident;
        let symbol = format!("{}{name}", prefix.value());
        // The delimiter makes ordered opcode names unambiguous. This is an ABI
        // compatibility fingerprint, not an adversarial integrity check.
        for byte in symbol
            .bytes()
            .chain(std::iter::once(OPCODE_NAME_TERMINATOR))
        {
            hash = (hash ^ u64::from(byte)).wrapping_mul(FNV1A_PRIME);
        }
        let export = quote! {
            #[unsafe(export_name = #symbol)]
            pub unsafe extern "C" fn #name(
                context: *mut ::std::ffi::c_void, pc: u64,
                operands: *const u64, result: *mut u64,
            ) -> i64 {
                // SAFETY: forwarded from the generated native entry ABI.
                unsafe { $crate::native_js_operation_entry::<$host, #index>(context, pc, operands, result) }
            }
        };
        export_arms.push(quote! { (@operation $host:ty, #symbol) => { #export }; });
        exports.push(export);
    }
    let marker = format!("quench_native_js_abi_v{version}_{hash:016x}");
    Ok(quote! {
        #operations
        #[cfg(feature = "native-execution")]
        pub const NATIVE_JS_OPERATION_PREFIX: &str = #prefix;
        #[cfg(feature = "native-execution")]
        pub const NATIVE_JS_ABI_SYMBOL: &str = #marker;
        /// The header contains the ABI marker followed by the optional region count.
        #[cfg(feature = "native-execution")]
        pub const NATIVE_JS_IMAGE_HEADER_WORDS: usize = 2;

        /// Export the derived semantic helpers for one host in an executable.
        /// Invoke once, using a host type path and a module name for the exports.
        #[cfg(feature = "native-execution")]
        #[macro_export]
        macro_rules! export_native_js_operations {
            ($host:ty, $module:ident) => {
                #[allow(non_snake_case)]
                mod $module {
                    #[allow(unused_imports)]
                    use super::*;
                    #[unsafe(export_name = #marker)]
                    pub extern "C" fn abi_marker() {}
                    #(#exports)*
                }
            };
            ($host:ty, $module:ident, [$($operation:tt),* $(,)?]) => {
                #[allow(non_snake_case)]
                mod $module {
                    #[allow(unused_imports)]
                    use super::*;
                    $( $crate::export_native_js_operations!(@operation $host, $operation); )*
                }
            };
            (@operation $host:ty, #marker) => {
                #[unsafe(export_name = #marker)]
                pub extern "C" fn abi_marker() {}
            };
            #(#export_arms)*
        }
    })
}

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
        #visibility const fn #selector(op: Op) -> fn(#(#types),*) #output {
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
    fn native_exports_reject_non_contiguous_or_conditional_variants() {
        for enumeration in [
            syn::parse_quote!(
                enum Op {
                    First = 7,
                    Last,
                }
            ),
            syn::parse_quote!(
                enum Op {
                    First,
                    #[cfg(any())]
                    Last,
                }
            ),
            syn::parse_quote!(
                enum Op {
                    First(u32),
                    Last,
                }
            ),
        ] {
            assert!(
                derive_native_exports(
                    NativeExportLayout {
                        version: syn::parse_quote!(1),
                        prefix: syn::parse_quote!("operation_"),
                    },
                    enumeration
                )
                .is_err()
            );
        }
    }

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
