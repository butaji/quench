//! Pattern generation

use crate::transpile::codegen::CodeGenerator;
use crate::transpile::hir::*;

pub struct PatGen;

impl PatGen {
    pub fn pat_to_rust(cg: &CodeGenerator, pat: &Pat, source_name: &str) -> Vec<String> { let mut lines = Vec::new(); Self::pat_to_rust_impl(cg, pat, source_name, &mut lines); lines }

    fn pat_to_rust_impl(cg: &CodeGenerator, pat: &Pat, source_name: &str, lines: &mut Vec<String>) {
        use Pat::*;
        match pat {
            Ident { name, type_ } => { let t = type_.as_ref().map(|t| format!(": {}", cg.type_to_rust(t))).unwrap_or_default(); lines.push(format!("let {}{} = {};", name, t, source_name)); }
            Object { props, .. } => { for prop in props { match prop { ObjectPatProp::Init { key, value } => lines.push(format!("let {:?} = {}.{};", value, source_name, key)), ObjectPatProp::Rest { arg } => Self::pat_to_rust_impl(cg, arg, &format!("&{}[..]", source_name), lines), } } }
            Array { elems, .. } => { for (i, elem) in elems.iter().enumerate() { if let Some(pat) = elem { Self::pat_to_rust_impl(cg, pat, &format!("{}[{}]", source_name, i), lines); } } }
            Rest { arg } => Self::pat_to_rust_impl(cg, arg, &format!("&{}[..]", source_name), lines),
            Assign { left, .. } => Self::pat_to_rust_impl(cg, left, source_name, lines),
            Default { arg, .. } => Self::pat_to_rust_impl(cg, arg, source_name, lines),
        }
    }
}
