//! One semantics per opcode: the interpreter's handler defines it and a native lowering only
//! specialises it. Two measures keep native code from drifting into its own semantics:
//! - `native_form <Class>`: an opcode family of the catalog (`bytecode::op_catalog::Class`)
//!   that no lane lowering (`vm/compiled/lane_region*`) names, so every instruction of it is
//!   handed back to the interpreter at compile time. Wasm families belong to the Wasm lane.
//! - `native_refusals`: the refusal sites the lane's call and construction helpers declare in
//!   `vm/lane_body_stats.rs` (`miss_reasons!`); each is a case the handler resolves and the
//!   helper declines.
//!
//! Both join the shrink-only measures in `oversized.txt`; a lowering or a refusal that goes
//! away shrinks them, one that appears fails.

use std::{collections::BTreeSet, fs, path::PathBuf};

use syn::visit::Visit;

use crate::shape::Measures;

const CATALOG: &str = "bytecode/op_catalog.rs";
const LANE_LOWERING: &str = "vm/compiled/lane_region";
const REFUSALS: &str = "vm/lane_body_stats.rs";
/// Families that need no native form: the catalog's catch-all and the Wasm lane's own.
const EXEMPT: &[&str] = &["Other", "Nop"];
const EXEMPT_PREFIX: &str = "Wasm";

/// Records the native-form measures of the files, which read as `native_form <Class>`
/// and `native_refusals`.
pub fn scan(files: &[PathBuf], measures: &mut Measures) {
    let mut families = BTreeSet::new();
    let mut named = BTreeSet::new();
    for path in files {
        let file = path.display().to_string();
        let relevant = file.ends_with(CATALOG) || file.contains(LANE_LOWERING) || file.ends_with(REFUSALS);
        let Some(source) = relevant.then(|| fs::read_to_string(path).ok()).flatten() else { continue };
        if file.ends_with(REFUSALS) {
            let declared = source.split("miss_reasons! {").nth(1).and_then(|rest| rest.split("\n}").next())
                .map_or(0, |body| body.lines().filter(|line| line.contains("=>")).count());
            measures.insert("native_refusals".to_string(), declared);
            continue;
        }
        let Ok(syntax) = syn::parse_file(&source) else { continue };
        if file.ends_with(CATALOG) {
            syntax.items.iter().for_each(|item| {
                if let syn::Item::Enum(class) = item && class.ident == "Class" {
                    class.variants.iter().for_each(|variant| { families.insert(variant.ident.to_string()); });
                }
            });
        } else {
            let mut names = ClassNames(&mut named);
            names.visit_file(&syntax);
        }
    }
    families.iter()
        .filter(|family| !EXEMPT.contains(&family.as_str()) && !family.starts_with(EXEMPT_PREFIX))
        .filter(|family| !named.contains(*family))
        .for_each(|family| { measures.insert(format!("native_form {family}"), 1); });
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
