//! Pattern generation

use crate::transpile::codegen::CodeGenerator;
use crate::transpile::hir::*;

pub struct PatGen;

impl PatGen {
    pub fn pat_to_rust(cg: &CodeGenerator, pat: &Pat, source_name: &str) -> Vec<String> {
        let mut lines = Vec::new();
        Self::pat_to_rust_impl(cg, pat, source_name, &mut lines);
        lines
    }
    fn pat_to_rust_impl(cg: &CodeGenerator, pat: &Pat, source_name: &str, lines: &mut Vec<String>) {
        use Pat::*;
        let t = pat;
        if let Ident { name, type_ } = t {
            let ty = type_
                .as_ref()
                .map(|t| format!(": {}", cg.type_to_rust(t)))
                .unwrap_or_default();
            lines.push(format!("let {}{} = {};", name, ty, source_name));
            return;
        }
        if let Object { props, .. } = t {
            for prop in props {
                Self::object_prop_to_rust(prop, source_name, lines);
            }
            return;
        }
        if let Array { elems, .. } = t {
            for (i, elem) in elems.iter().enumerate() {
                if let Some(p) = elem {
                    Self::pat_to_rust_impl(cg, p, &format!("{}[{}]", source_name, i), lines);
                }
            }
            return;
        }
        if let Rest { arg } = t {
            Self::pat_to_rust_impl(cg, arg, &format!("&{}[..]", source_name), lines);
            return;
        }
        if let Assign { left, .. } = t {
            Self::pat_to_rust_impl(cg, left, source_name, lines);
        }
    }
    fn object_prop_to_rust(prop: &ObjectPatProp, source: &str, lines: &mut Vec<String>) {
        if let ObjectPatProp::Init { key, value } = prop {
            lines.push(format!("let {:?} = {}.{};", value, source, key));
        } else if let ObjectPatProp::Rest { arg } = prop {
            lines.push(format!("// rest: {:?}", arg));
        }
    }
}
