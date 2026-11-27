//! Physical code attribution before symbol removal. Spans include alignment bytes;
//! they describe retention, never prove that a semantic operation is unnecessary.

use super::link::audit::{INTERPRETER_DISPATCH_SYMBOLS, compiler_symbol_patterns};
use object::{Object, ObjectSection, ObjectSymbol, SectionKind};
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
    /// Code bytes by retention mechanism. The mechanisms partition `code_bytes`.
    pub retained_mechanisms: BTreeMap<String, RetainedMechanism>,
    pub attribution: &'static str,
}

/// One mechanism's bytes, and its largest symbol: the representative that `tools/image-why-live.sh`
/// asks the linker about.
#[derive(Serialize)]
pub struct RetainedMechanism {
    pub bytes: u64,
    /// The demangled name of the mechanism's largest symbol.
    pub representative: String,
    /// That symbol's name in the object files, the form `--why-live` takes; empty when the image
    /// names nothing at the representative's address.
    pub linker_symbol: String,
    pub address: u64,
}

struct Symbol<'a> { address: u64, owner: Owner<'a>, mechanism: &'static str, name: &'a str }

/// The code a span is attributed to: the symbol that starts it, or the gap before the first one.
#[derive(Clone, Copy)]
struct Current<'a> { owner: Owner<'a>, mechanism: &'static str, address: u64, name: &'a str }

/// One mechanism's running total, with its largest span.
#[derive(Default)]
struct Retained<'a> { bytes: u64, largest: u64, address: u64, name: &'a str }

/// Reported, not refused: the link audit refuses the interpreter and the compiler (its own
/// patterns, reused here), so these name the rest of what an image may retain.
const COMPILER_EXTRA_PATTERNS: &[&str] = &["compile::", "specializer"];
const MECHANISM_RULES: &[(&str, &[&str])] = &[
    ("live_linker", &["aot_image::live", "aot_image::file", "code_heap", "install_ahead_body", "link_lane"]),
    ("tier", &["tier_policy", "tier_machine", "code_dependencies"]),
    ("bytecode_decode", &["from_binary_bytes", "read_binary", "decode_binary"]),
    ("natives", &["NativeFamily", "call_native_dispatch", "native_families", "natives::"]),
    ("realm_installers", &["install_"]),
    ("intl", &["quench_runtime_intl", "icu_"]),
    ("tz", &["chrono_tz", "tz_data"]),
    ("regexp_compiler", &["quench_regexp"]),
    ("wasm", &["wasmparser", "wasm::"]),
    ("std_fmt_panic", &["core::fmt", "alloc::fmt", "std::fmt", "core::panicking", "std::panicking", "unwrap_failed", "expect_failed"]),
];

/// The first mechanism whose pattern the demangled symbol names; `other` for code none claims.
fn mechanism(name: &str) -> &'static str {
    if INTERPRETER_DISPATCH_SYMBOLS.iter().any(|pattern| name.contains(pattern)) || name.contains("dispatch_fast") {
        return "interpreter";
    }
    if compiler_symbol_patterns(name).next().is_some() || COMPILER_EXTRA_PATTERNS.iter().any(|pattern| name.contains(pattern)) {
        return "compiler";
    }
    MECHANISM_RULES.iter()
        .find(|(_, patterns)| patterns.iter().any(|pattern| name.contains(pattern)))
        .map_or("other", |(mechanism, _)| *mechanism)
}

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
            let name = name.trim_start();
            symbols.push(Symbol { address, owner: owner(name), mechanism: mechanism(name), name });
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

fn attribute<'a>(
    owners: &mut BTreeMap<Owner<'a>, u64>,
    mechanisms: &mut BTreeMap<&'static str, Retained<'a>>,
    current: Current<'a>,
    bytes: u64,
) {
    *owners.entry(current.owner).or_default() += bytes;
    let retained = mechanisms.entry(current.mechanism).or_default();
    retained.bytes += bytes;
    if bytes > retained.largest {
        retained.largest = bytes;
        retained.address = current.address;
        retained.name = current.name;
    }
}

fn spans(sections: &[(u64, u64)], symbols: &[Symbol<'_>]) -> Result<NativeImageFootprint, String> {
    let mut owners = BTreeMap::<Owner<'_>, u64>::new();
    let mut mechanisms = BTreeMap::<&'static str, Retained<'_>>::new();
    let mut code_bytes = 0_u64;
    for &(start, length) in sections {
        let end = start.checked_add(length).ok_or("native code section address overflow")?;
        code_bytes = code_bytes.checked_add(length).ok_or("native code size overflow")?;
        let mut cursor = start;
        let mut current = Current { owner: Owner::Unattributed, mechanism: "unattributed", address: start, name: "" };
        let begin = symbols.partition_point(|symbol| symbol.address < start);
        for symbol in symbols[begin..].iter().take_while(|symbol| symbol.address < end) {
            if symbol.address != cursor {
                attribute(&mut owners, &mut mechanisms, current, symbol.address - cursor);
            }
            cursor = symbol.address;
            current = Current { owner: symbol.owner, mechanism: symbol.mechanism, address: symbol.address, name: symbol.name };
        }
        if end != cursor { attribute(&mut owners, &mut mechanisms, current, end - cursor); }
    }
    Ok(NativeImageFootprint {
        code_bytes,
        code_spans_by_symbol_root: owners.into_iter().map(|(owner, bytes)| (owner.label(), bytes)).collect(),
        retained_mechanisms: mechanisms.into_iter().map(|(mechanism, retained)| (mechanism.to_owned(), RetainedMechanism {
            bytes: retained.bytes,
            representative: retained.name.to_owned(),
            linker_symbol: String::new(),
            address: retained.address,
        })).collect(),
        attribution: "demangled outer-symbol-name roots, including alignment; aliases counted once; not defining-crate provenance or a semantic reachability proof",
    })
}

/// The linker's name of each code address (`--why-live` takes linker names, which `nm -C` hides).
fn linker_names(image: &object::File<'_>) -> BTreeMap<u64, String> {
    let mut names = BTreeMap::new();
    for symbol in image.symbols() {
        if let Ok(name) = symbol.name().map(str::to_owned) {
            if !name.is_empty() { names.entry(symbol.address()).or_insert(name); }
        }
    }
    names
}

pub(super) fn read(executable: &Path, names: &str) -> Result<NativeImageFootprint, String> {
    let bytes = fs::read(executable).map_err(|error| error.to_string())?;
    let image = object::File::parse(bytes.as_slice()).map_err(|error| error.to_string())?;
    let sections: Vec<_> = image.sections().filter(|section| section.kind() == SectionKind::Text)
        .map(|section| (section.address(), section.size())).collect();
    let mut footprint = spans(&sections, &symbols(names))?;
    let linker_names = linker_names(&image);
    for retained in footprint.retained_mechanisms.values_mut() {
        retained.linker_symbol = linker_names.get(&retained.address).cloned().unwrap_or_default();
    }
    Ok(footprint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribution_counts_aliases_once_and_does_not_cross_section_gaps() {
        let names = "00000067 T quench::first\n000000c8 t quench_runtime::second\n\
            000000d2 T quench::alias\n000000d2 W other::alias\n\
            000000d2 T quench::alias_again\n0000012c D data::value\n U imported";
        let report = spans(&[(100, 10), (200, 20)], &symbols(names)).unwrap();
        assert_eq!(report.code_bytes, 30);
        assert_eq!(report.code_spans_by_symbol_root, BTreeMap::from([
            ("unattributed".into(), 3), ("symbol_root:quench".into(), 7),
            ("symbol_root:quench_runtime".into(), 10), ("shared_symbol_address".into(), 10),
        ]));
        assert_eq!(report.code_spans_by_symbol_root.values().sum::<u64>(), report.code_bytes);
    }

    #[test]
    fn mechanisms_partition_code_and_name_their_largest_symbol() {
        let names = "00000064 T quench_runtime_engine::dispatch_fast::run_fast_lane\n\
            000000c8 t quench_runtime::aot_image::live::link_lane\n\
            000000d2 T quench_runtime::vm::builtins::install_builtins\n";
        let report = spans(&[(100, 10), (200, 20)], &symbols(names)).unwrap();
        assert_eq!(report.retained_mechanisms["interpreter"].bytes, 10);
        assert_eq!(report.retained_mechanisms["live_linker"].bytes, 10);
        assert_eq!(report.retained_mechanisms["realm_installers"].bytes, 10);
        assert_eq!(report.retained_mechanisms["live_linker"].representative, "quench_runtime::aot_image::live::link_lane");
        assert_eq!(report.retained_mechanisms["live_linker"].address, 200);
        let partitioned: u64 = report.retained_mechanisms.values().map(|retained| retained.bytes).sum();
        assert_eq!(partitioned, report.code_bytes);
    }

    #[test]
    fn generic_type_arguments_do_not_reassign_the_outer_symbol_root() {
        assert!(owner("<quench_runtime::vm::Vm<quench::NodeHost>>::call") == Owner::SymbolRoot("quench_runtime"));
        assert!(owner("core::ptr::drop_in_place<quench::Host>") == Owner::SymbolRoot("core"));
        assert!(owner("_quench_js_op_Call") == Owner::SemanticAbi);
        assert!(owner("_main") == Owner::NativeOrUnclassified);
        let empty = spans(&[(10, 5)], &[]).unwrap();
        assert_eq!(empty.code_spans_by_symbol_root["unattributed"], 5);
        assert_eq!(empty.retained_mechanisms["unattributed"].bytes, 5);
        assert!(spans(&[(u64::MAX, 1)], &[]).is_err());
    }
}
