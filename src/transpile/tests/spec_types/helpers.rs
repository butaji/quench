//! Helper functions for type tests

pub use crate::transpile::hir::*;
pub use crate::transpile::hir::type_to_rust::{TypeToRust, OutputKind, RustType};
pub use crate::transpile::parser::TsParser;
pub use proc_macro2::TokenStream;
pub use quote::ToTokens;

// =========================================================================
// Helper functions
// =========================================================================

pub fn parse_fn_return_type(source: &str) -> Option<Type> {
    let parser = TsParser::new();
    let result = parser.parse_source(source).ok()?;
    for item in &result.items {
        if let ModuleItem::Decl(Decl::Function(f)) = item {
            return f.return_type.clone();
        }
    }
    None
}

pub fn parse_fn_param_type(source: &str, param_idx: usize) -> Option<Type> {
    let parser = TsParser::new();
    let result = parser.parse_source(source).ok()?;
    for item in &result.items {
        if let ModuleItem::Decl(Decl::Function(f)) = item {
            return f.params.get(param_idx).and_then(|p| p.type_.clone());
        }
    }
    None
}

pub fn type_to_rust_name(ty: &Type) -> String {
    let converter = TypeToRust::new(OutputKind::String);
    converter.convert(ty).type_name()
}

pub fn codegen_produces_output(ty: &Type) -> bool {
    let cg = QuoteCodegen::default();
    let tokens = cg.gen_type(ty);
    !tokens.is_empty()
}

pub fn find_type_decl(source: &str, name: &str) -> Option<TypeDecl> {
    let parser = TsParser::new();
    let result = parser.parse_source(source).ok()?;
    for item in &result.items {
        if let ModuleItem::Decl(Decl::Type(t)) = item {
            if t.name == name {
                return Some(t.clone());
            }
        }
    }
    None
}
