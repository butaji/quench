//! One semantics per opcode: the interpreter's handler defines it and a native lowering only
//! specialises it. Two measures keep native code from drifting into its own semantics:
//! - `native_form <Class>`: an opcode family of the catalog (`bytecode::op_catalog::Class`)
//!   that no lane lowering (`vm/compiled/lane_region*`) names, so every instruction of it is
//!   handed back to the interpreter at compile time. Wasm families belong to the Wasm lane.
//! - `native_refusals`: the refusal sites the lane's call and construction helpers declare in
//!   `vm/lane_body_stats.rs` (`miss_reasons!`); each is a case the handler resolves and the
//!   helper declines.
//!
//! - `native_ir <file>`: instructions a JIT file (`vm/compiled/`, outside tests) builds by hand
//!   (`.ins()`), the volume of operation meaning written beside the handler it mirrors rather
//!   than derived from the staged operation (`staged/`). Each is a hand lowering to move
//!   into the operation table or replace by a call of the shared slow path.
//!
//! All join the shrink-only measures in `oversized.txt`; a lowering or a refusal that goes
//! away shrinks them, one that appears fails.

use std::{collections::BTreeSet, fs, path::PathBuf};

use syn::visit::Visit;

use crate::shape::Measures;

const CATALOG: &str = "bytecode/op_catalog.rs";
const LANE_LOWERING: &str = "vm/compiled/lane_region";
/// Where the JIT builds native code by hand.
const JIT_FILES: &str = "vm/compiled/";
const REFUSALS: &str = "vm/lane_body_stats.rs";
/// Families that need no native form: the catalog's catch-all and the Wasm lane's own.
const EXEMPT: &[&str] = &["Other", "Nop"];
const EXEMPT_PREFIX: &str = "Wasm";

/// Records the native-form measures of the files: `native_form <Class>`, `native_refusals`
/// and `native_ir <file>`.
pub fn scan(files: &[PathBuf], measures: &mut Measures) {
    let mut families = BTreeSet::new();
    let mut named = BTreeSet::new();
    for path in files {
        let file = path.display().to_string();
        hand_built(&file, path, measures);
        let relevant = file.ends_with(CATALOG) || file.contains(LANE_LOWERING) || file.ends_with(REFUSALS);
        let Some(source) = relevant.then(|| fs::read_to_string(path).ok()).flatten() else { continue };
        if file.ends_with(REFUSALS) {
            measures.insert("native_refusals".to_string(), refusals(&source));
        } else if let Ok(syntax) = syn::parse_file(&source) {
            if file.ends_with(CATALOG) { families.extend(class_variants(&syntax)); named.extend(generic_steps(&source)); } else { ClassNames(&mut named).visit_file(&syntax); }
        }
    }
    families.iter()
        .filter(|family| !EXEMPT.contains(&family.as_str()) && !family.starts_with(EXEMPT_PREFIX))
        .filter(|family| !named.contains(*family))
        .for_each(|family| { measures.insert(format!("native_form {family}"), 1); });
}

/// The instructions a JIT file builds by hand (`native_ir <file>`).
fn hand_built(file: &str, path: &PathBuf, measures: &mut Measures) {
    if !file.contains(JIT_FILES) || crate::in_test_path(file) || file.ends_with("tests.rs") { return; }
    let built = fs::read_to_string(path).map_or(0, |source| source.matches(".ins()").count());
    if built > 0 { measures.insert(format!("native_ir {file}"), built); }
}

/// The refusal sites `miss_reasons!` declares.
fn refusals(source: &str) -> usize {
    source.split("miss_reasons! {").nth(1).and_then(|rest| rest.split("\n}").next())
        .map_or(0, |body| body.lines().filter(|line| line.contains("=>")).count())
}

/// The families the catalog gives the generic native form of a value step (`LANE_RESULT_STEP`,
/// `Vm::call_out`), each lowered by one helper of the interpreter's definition.
fn generic_steps(catalog: &str) -> Vec<String> {
    catalog.lines().filter(|line| line.contains("LANE_RESULT_STEP") && line.contains("=>"))
        .filter_map(|line| Some(line.split_once('(')?.1.split_once(')')?.0.trim().to_string())).collect()
}

/// The families of the catalog's `Class`.
fn class_variants(syntax: &syn::File) -> Vec<String> {
    syntax.items.iter().filter_map(|item| match item {
        syn::Item::Enum(class) if class.ident == "Class" => Some(class.variants.iter().map(|variant| variant.ident.to_string())),
        _ => None,
    }).flatten().collect()
}

/// The `Class::Name` variants a lowering mentions.
struct ClassNames<'a>(&'a mut BTreeSet<String>);

impl<'ast> Visit<'ast> for ClassNames<'_> {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        let segments: Vec<_> = path.segments.iter().collect();
        if let [.., class, variant] = segments.as_slice() && class.ident == "Class" {
            self.0.insert(variant.ident.to_string());
        }
        syn::visit::visit_path(self, path);
    }
}

/// Adds the native-form measures the baseline has never recorded to `baseline`.
pub fn first_baseline(found: &Measures, recorded: &Measures, baseline: &mut Measures) {
    found.iter()
        .filter(|(subject, _)| subject.starts_with("native_") && !recorded.contains_key(*subject))
        .for_each(|(subject, count)| { baseline.insert(subject.clone(), *count); });
}
