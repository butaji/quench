//! One home for fact types: a struct or enum named `*Fact` or `*Facts` in the runtime crates
//! belongs to `quench_runtime::typed_ir::cell`, the one lattice cell every fact migrates onto
//! (R1, TOD-2448). The types that predate the rule are listed in `fact_types.txt`, one
//! `path::TypeName` per line, and the list only shrinks: an unlisted type fails, and an entry
//! whose type no longer exists fails until its line is deleted together with the code.

use std::{collections::BTreeSet, fs};

use syn::visit::Visit;

use crate::shape::is_test;

/// The one home of fact types; the rule does not govern it.
pub const HOME: &str = "crates/quench-runtime/src/typed_ir/cell.rs";
/// The source trees the rule governs.
const SCOPES: &[&str] = &["crates/quench-runtime/src/", "crates/quench-runtime-engine/src/"];
/// The name endings that mark a fact type.
const SUFFIXES: &[&str] = &["Fact", "Facts"];
const ALLOWLIST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fact_types.txt");

/// The fact types a file declares outside the home and outside test code: empty for a file the
/// rule does not govern.
pub fn outside_home(file: &str, syntax: &syn::File) -> Vec<String> {
    let governed = SCOPES.iter().any(|scope| file.contains(scope)) && file != HOME;
    if !governed || crate::test_paths::in_test_path(file) {
        return Vec::new();
    }
    declared(syntax)
}

/// The names of the structs and enums of a file that end in `Fact` or `Facts`, found anywhere
/// in the file (inline modules and function bodies included) except in test items.
pub fn declared(syntax: &syn::File) -> Vec<String> {
    let mut found = Declared(Vec::new());
    found.visit_file(syntax);
    found.0
}

/// The `path::TypeName` keys of the fact types a scanned file declares, collected for the list.
pub fn record(file: &str, syntax: &syn::File, found: &mut BTreeSet<String>) {
    found.extend(outside_home(file, syntax).into_iter().map(|name| format!("{file}::{name}")));
}

/// Prints each departure from `fact_types.txt`; returns how many there are.
pub fn compare(found: &BTreeSet<String>) -> usize {
    let listing = fs::read_to_string(ALLOWLIST).unwrap_or_default();
    let recorded: BTreeSet<&str> =
        listing.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')).collect();
    let mut departures = 0;
    for key in found.iter().filter(|key| !recorded.contains(key.as_str())) {
        println!("fact-types: {key} is a fact type outside {HOME}; declare it as a Cell, Classes, Elements or FactKey of quench_runtime::typed_ir instead");
        departures += 1;
    }
    for key in recorded.iter().filter(|key| !found.contains(**key)) {
        println!("fact-types: {key} no longer exists; delete its line from crates/quench-lint/fact_types.txt");
        departures += 1;
    }
    departures
}

/// The fact-type names a file declares, outside test items.
struct Declared(Vec<String>);

impl Declared {
    fn name(&mut self, attrs: &[syn::Attribute], ident: &syn::Ident) {
        let name = ident.to_string();
        if !is_test(attrs) && SUFFIXES.iter().any(|suffix| name.ends_with(suffix)) {
            self.0.push(name);
        }
    }
}

impl<'ast> Visit<'ast> for Declared {
    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        self.name(&item.attrs, &item.ident);
    }

    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        self.name(&item.attrs, &item.ident);
    }

    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if !is_test(&module.attrs) {
            syn::visit::visit_item_mod(self, module);
        }
    }

    fn visit_item_fn(&mut self, function: &'ast syn::ItemFn) {
        if !is_test(&function.attrs) {
            syn::visit::visit_item_fn(self, function);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> syn::File {
        syn::parse_file(source).expect("test source parses")
    }

    #[test]
    fn fact_types_are_structs_and_enums_named_for_facts_outside_test_items() {
        let file = parse(
            "struct IntFact(()); pub enum ExitFacts { A } struct Facter; struct FactTable;
             mod m { pub(crate) struct ReadFact; }
             fn f() { struct NestedFact; }
             #[cfg(test)] mod tests { struct TestFact; }
             #[cfg(test)] struct GatedFacts;
             #[cfg(test)] fn g() { struct GatedInFn; }",
        );
        assert_eq!(declared(&file), ["IntFact", "ExitFacts", "ReadFact", "NestedFact"]);
    }

    #[test]
    fn the_home_and_ungoverned_files_declare_nothing_to_list() {
        let file = parse("struct IntFact(()); struct PlainFact;");
        assert!(outside_home(HOME, &file).is_empty());
        assert!(outside_home("crates/quench-lint/src/x.rs", &file).is_empty());
        assert_eq!(outside_home("crates/quench-runtime/src/staged/proof.rs", &file), ["IntFact", "PlainFact"]);
    }
}
