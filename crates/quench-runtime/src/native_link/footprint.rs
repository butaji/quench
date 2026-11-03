//! Physical code attribution before symbol removal. Spans include alignment bytes;
//! they describe retention, never prove that a semantic operation is unnecessary.

use object::{Object, ObjectSection, SectionKind};
use serde::Serialize;
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Owner<'a> { SymbolRoot(&'a str), SemanticAbi, NativeOrUnclassified, Unattributed, SharedAddress }

impl Owner<'_> {
    fn label(self) -> String {
        match self {
            Self::SymbolRoot(name) => format!("symbol_root:{name}"),
            Self::SemanticAbi => "semantic_abi".into(),
            Self::NativeOrUnclassified => "native_or_unclassified".into(),
            Self::Unattributed => "unattributed".into(),
            Self::SharedAddress => "shared_symbol_address".into(),
        }
    }
}

#[derive(Serialize)]
pub struct NativeImageFootprint {
    pub code_bytes: u64,
    pub code_spans_by_symbol_root: BTreeMap<String, u64>,
    pub attribution: &'static str,
}

struct Symbol<'a> { address: u64, owner: Owner<'a> }

fn owner(name: &str) -> Owner<'_> {
    let name = name.trim_start_matches('_').trim_start_matches('<');
    if let Some((root, _)) = name.split_once("::") {
        // Group the outer displayed namespace, not names in generic arguments.
        // Trait implementations and inlining need not originate in this namespace.
        if !root.is_empty() && root.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') {
            return Owner::SymbolRoot(root);
        }
    }
    if name.starts_with(crate::NATIVE_JS_OPERATION_PREFIX) { Owner::SemanticAbi }
    else { Owner::NativeOrUnclassified }
}

fn symbols(text: &str) -> Vec<Symbol<'_>> {
    let mut symbols = Vec::new();
    for line in text.lines() {
        let line = line.trim_start();
        let Some((address, rest)) = line.split_once(char::is_whitespace) else { continue };
        let Ok(address) = u64::from_str_radix(address, 16) else { continue };
        let rest = rest.trim_start();
        let Some((kind, name)) = rest.split_once(char::is_whitespace) else { continue };
        if matches!(kind, "T" | "t" | "W" | "w") {
            symbols.push(Symbol { address, owner: owner(name.trim_start()) });
        }
    }
    symbols.sort_unstable_by_key(|symbol| symbol.address);
    // Aliases share one physical span. Conflicting owners remain explicitly shared.
    let mut unique: Vec<Symbol<'_>> = Vec::with_capacity(symbols.len());
    for symbol in symbols {
        if let Some(last) = unique.last_mut().filter(|last| last.address == symbol.address) {
            if last.owner != symbol.owner { last.owner = Owner::SharedAddress; }
        } else { unique.push(symbol); }
    }
    unique
}

fn spans(sections: &[(u64, u64)], symbols: &[Symbol<'_>]) -> Result<NativeImageFootprint, String> {
    let mut owners = BTreeMap::<Owner<'_>, u64>::new();
    let mut code_bytes = 0_u64;
    for &(start, length) in sections {
        let end = start.checked_add(length).ok_or("native code section address overflow")?;
        code_bytes = code_bytes.checked_add(length).ok_or("native code size overflow")?;
        let mut cursor = start;
        let mut current = Owner::Unattributed;
        let begin = symbols.partition_point(|symbol| symbol.address < start);
        for symbol in symbols[begin..].iter().take_while(|symbol| symbol.address < end) {
            if symbol.address != cursor {
                *owners.entry(current).or_default() += symbol.address - cursor;
            }
            cursor = symbol.address;
            current = symbol.owner;
        }
        if end != cursor { *owners.entry(current).or_default() += end - cursor; }
    }
    Ok(NativeImageFootprint { code_bytes, code_spans_by_symbol_root: owners.into_iter().map(|(owner, bytes)| (owner.label(), bytes)).collect(),
        attribution: "demangled outer-symbol-name roots, including alignment; aliases counted once; not defining-crate provenance or a semantic reachability proof" })
}

pub(super) fn read(executable: &Path, names: &str) -> Result<NativeImageFootprint, String> {
    let bytes = fs::read(executable).map_err(|error| error.to_string())?;
    let image = object::File::parse(bytes.as_slice()).map_err(|error| error.to_string())?;
    let sections: Vec<_> = image.sections().filter(|section| section.kind() == SectionKind::Text)
        .map(|section| (section.address(), section.size())).collect();
    spans(&sections, &symbols(names))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribution_counts_aliases_once_and_does_not_cross_section_gaps() {
        let names = "00000067 T quench_node::first\n000000c8 t quench_runtime::second\n\
            000000d2 T quench_node::alias\n000000d2 W other::alias\n\
            000000d2 T quench_node::alias_again\n0000012c D data::value\n U imported";
        let report = spans(&[(100, 10), (200, 20)], &symbols(names)).unwrap();
        assert_eq!(report.code_bytes, 30);
        assert_eq!(report.code_spans_by_symbol_root, BTreeMap::from([
            ("unattributed".into(), 3), ("symbol_root:quench_node".into(), 7),
            ("symbol_root:quench_runtime".into(), 10), ("shared_symbol_address".into(), 10),
        ]));
        assert_eq!(report.code_spans_by_symbol_root.values().sum::<u64>(), report.code_bytes);
    }

    #[test]
    fn generic_type_arguments_do_not_reassign_the_outer_symbol_root() {
        assert!(owner("<quench_runtime::vm::Vm<quench_node::NodeHost>>::call") == Owner::SymbolRoot("quench_runtime"));
        assert!(owner("core::ptr::drop_in_place<quench_node::Host>") == Owner::SymbolRoot("core"));
        assert!(owner("_quench_js_op_Call") == Owner::SemanticAbi);
        assert!(owner("_main") == Owner::NativeOrUnclassified);
        let empty = spans(&[(10, 5)], &[]).unwrap();
        assert_eq!(empty.code_spans_by_symbol_root["unattributed"], 5);
        assert!(spans(&[(u64::MAX, 1)], &[]).is_err());
    }
}
