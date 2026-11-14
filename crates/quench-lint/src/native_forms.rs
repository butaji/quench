//! One semantics per opcode: the interpreter's handler defines it and a native lowering only
//! specialises it. Two measures keep native code from drifting into its own semantics:
//! - `native_form <Class>`: an opcode family of the catalog (`bytecode::op_catalog::Class`)
//!   that no lane lowering (`vm/compiled/lane_region*`) names, so every instruction of it is
//!   handed back to the interpreter at compile time. Wasm families belong to the Wasm lane.
//!   The families the catalog gives a generic value step (`step LaneResult`, read from the same
//!   catalog rows as the census, `catalog::table`) need no lowering of their own.
//! - `native_refusals`: the refusal sites the lane's call and construction helpers declare in
//!   `vm/lane_body_stats.rs` (`miss_reasons!`); each is a case the handler resolves and the
//!   helper declines.
//!
//! - `native_ir <file>`: instructions a JIT file (`vm/compiled.rs`, `vm/compiled/`, outside
//!   tests) builds by hand (`.ins()`), the volume of operation meaning written beside the
//!   handler it mirrors rather than derived from the staged operation (`staged/`). Each is a
//!   hand lowering to move into the operation table or replace by a call of the shared slow
//!   path.
//!
//! All join the shrink-only measures in `oversized.txt`; a lowering or a refusal that goes
//! away shrinks them, one that appears fails.

use std::{collections::BTreeSet, fs, path::PathBuf};

use proc_macro2::{TokenStream, TokenTree};
use syn::visit::Visit;

use crate::catalog;
use crate::shape::{Measures, is_test};

const CATALOG: &str = "bytecode/op_catalog.rs";
/// The file of the `opcodes!` table.
const TABLE: &str = "quench-runtime/src/bytecode.rs";
const LANE_LOWERING: &str = "vm/compiled/lane_region";
/// Where the JIT builds native code by hand: the directory of its files, and the file beside it.
const JIT_DIRECTORY: &str = "vm/compiled/";
const JIT_ROOT: &str = "vm/compiled.rs";
const REFUSALS: &str = "vm/lane_body_stats.rs";
/// The method of the instruction builder.
const INSTRUCTION_BUILDER: &str = "ins";
/// Families that need no native form: the catalog's catch-all and the Wasm lane's own.
const EXEMPT: &[&str] = &["Other", "Nop"];
const EXEMPT_PREFIX: &str = "Wasm";

/// Whether the measures read `file`: the catalog's families and table, the lane lowerings and
/// the refusal declarations.
fn relevant(file: &str) -> bool {
    [CATALOG, TABLE, REFUSALS].iter().any(|name| file.ends_with(*name)) || file.contains(LANE_LOWERING)
}

/// Records the native-form measures of the files: `native_form <Class>`, `native_refusals`
/// and `native_ir <file>`.
pub fn scan(files: &[PathBuf], measures: &mut Measures) {
    let mut families = BTreeSet::new();
    let mut named = BTreeSet::new();
    for path in files {
        let file = path.display().to_string();
        hand_built(&file, path, measures);
        let Some(source) = relevant(&file).then(|| fs::read_to_string(path).ok()).flatten() else { continue };
        if file.ends_with(REFUSALS) {
            measures.insert("native_refusals".to_string(), refusals(&source));
        } else if let Ok(syntax) = syn::parse_file(&source) {
            catalog_facts(&file, &syntax, (&mut families, &mut named));
        }
    }
    families.iter()
        .filter(|family| !EXEMPT.contains(&family.as_str()) && !family.starts_with(EXEMPT_PREFIX))
        .filter(|family| !named.contains(*family))
        .for_each(|family| { measures.insert(format!("native_form {family}"), 1); });
}

/// Sorts what a parsed file contributes: the catalog's `Class` families, the classes its table
/// gives a generic step, or the classes a lane lowering names.
fn catalog_facts(file: &str, syntax: &syn::File, (families, named): (&mut BTreeSet<String>, &mut BTreeSet<String>)) {
    if file.ends_with(CATALOG) {
        families.extend(class_variants(syntax));
    } else if file.ends_with(TABLE) {
        named.extend(generic_steps(syntax));
    } else {
        ClassNames(named).visit_file(syntax);
    }
}

/// The instructions a JIT file builds by hand (`native_ir <file>`), outside test modules and
/// test paths.
fn hand_built(file: &str, path: &PathBuf, measures: &mut Measures) {
    let jit = file.contains(JIT_DIRECTORY) || file.ends_with(JIT_ROOT);
    if !jit || crate::in_test_path(file) { return; }
    let Some(syntax) = fs::read_to_string(path).ok().and_then(|source| syn::parse_file(&source).ok()) else { return };
    let mut built = Built(0);
    built.visit_file(&syntax);
    if built.0 > 0 { measures.insert(format!("native_ir {file}"), built.0); }
}

/// Counts `.ins()` builder calls, in code and in macro bodies, outside test modules and tests.
struct Built(usize);

impl<'ast> Visit<'ast> for Built {
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        self.0 += usize::from(call.method == INSTRUCTION_BUILDER && call.args.is_empty());
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_macro(&mut self, call: &'ast syn::Macro) {
        self.0 += builder_calls(call.tokens.clone());
    }

    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if !is_test(&module.attrs) { syn::visit::visit_item_mod(self, module); }
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if !is_test(&item.attrs) { syn::visit::visit_item_fn(self, item); }
    }
}

/// The `.ins()` calls among the tokens of a macro body.
fn builder_calls(tokens: TokenStream) -> usize {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let dotted = |at: usize| at > 0 && matches!(&trees[at - 1], TokenTree::Punct(punct) if punct.as_char() == '.');
    let empty = |at: usize| matches!(trees.get(at + 1), Some(TokenTree::Group(group)) if group.stream().is_empty());
    trees.iter().enumerate().map(|(at, tree)| match tree {
        TokenTree::Group(group) => builder_calls(group.stream()),
        TokenTree::Ident(name) => usize::from(name == INSTRUCTION_BUILDER && dotted(at) && empty(at)),
        _ => 0,
    }).sum()
}

/// The refusal sites `miss_reasons!` declares.
fn refusals(source: &str) -> usize {
    source.split("miss_reasons! {").nth(1).and_then(|rest| rest.split("\n}").next())
        .map_or(0, |body| body.lines().filter(|line| line.contains("=>")).count())
}

/// The families the catalog gives the generic native form of a value step (`step LaneResult`,
/// `Vm::call_out`), each lowered by one helper of the interpreter's definition.
fn generic_steps(syntax: &syn::File) -> Vec<String> {
    catalog::table(syntax).iter()
        .filter(|row| row.value(catalog::STEP) == Some(catalog::RESULT_STEP))
        .filter_map(|row| row.value(catalog::CLASS).map(str::to_owned))
        .collect()
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
