//! One semantics per opcode: the interpreter's handler defines it and a native lowering only
//! specialises it. Two measures keep native code from drifting into its own semantics:
//! - `native_form <Class>`: an opcode family of the catalog (`bytecode::op_catalog::Class`)
//!   that no lane lowering (`vm/compiled/lane_region*`) names, so every instruction of it is
//!   handed back to the interpreter at compile time. Wasm families belong to the Wasm lane.
//!   The families the catalog gives a generic value step (`step LaneResult`, read from the same
//!   catalog rows as the census, `catalog::table`) need no lowering of their own. A lowering
//!   that dispatches on a row's denotation (`JsDenotation::X`, the `js` column) names every
//!   family that row gives it, and a row of a lane decoder (`staged Flow|Value|Local`) or one
//!   `staged/allocation.rs` decodes is lowered through its staged operation.
//! - `native_refusals`: the refusal sites the lane's call and construction helpers declare in
//!   `vm/lane_body_stats.rs` (`miss_reasons!`); each is a case the handler resolves and the
//!   helper declines.
//!
//! - `native_ir <file>`: instructions a JIT file (`vm/compiled.rs`, `vm/compiled/`, outside
//!   tests) builds by hand (`.ins()`), the volume of operation meaning written beside the
//!   handler it mirrors rather than derived from the staged operation (`staged/`). Each is a
//!   hand lowering to move into the operation table or replace by a call of the shared slow
//!   path. `vm/compiled/emit_machine/` is the one sanctioned home of Cranelift instructions:
//!   it is the Machine primitive backend the derived layers emit through, so its `.ins()` are
//!   not counted; the measure is hand IR outside it.
//!
//! All join the shrink-only measures in `oversized.txt`; a lowering or a refusal that goes
//! away shrinks them, one that appears fails.

use std::{collections::{BTreeMap, BTreeSet}, fs, path::PathBuf};

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
/// The Machine primitive backend, the sanctioned home of the instruction builder.
const MACHINE_HOME: &str = "vm/compiled/emit_machine/";
/// The decoder of the allocation rows the lane lowers (`lane_region/allocations.rs`).
const ALLOCATION: &str = "staged/allocation.rs";
const REFUSALS: &str = "vm/lane_body_stats.rs";
/// The method of the instruction builder.
const INSTRUCTION_BUILDER: &str = "ins";
/// Families that need no native form: the catalog's catch-all and the Wasm lane's own.
const EXEMPT: &[&str] = &["Other", "Nop"];
const EXEMPT_PREFIX: &str = "Wasm";

/// Whether the measures read `file`: the catalog's families and table, the lane lowerings and
/// the refusal declarations.
fn relevant(file: &str) -> bool {
    [CATALOG, TABLE, REFUSALS, ALLOCATION].iter().any(|name| file.ends_with(*name)) || file.contains(LANE_LOWERING)
}

/// Records the native-form measures of the files: `native_form <Class>`, `native_refusals`
/// and `native_ir <file>`.
pub fn scan(files: &[PathBuf], measures: &mut Measures) {
    let mut facts = Facts::default();
    for path in files {
        let file = path.display().to_string();
        hand_built(&file, path, measures);
        let Some(source) = relevant(&file).then(|| fs::read_to_string(path).ok()).flatten() else { continue };
        if file.ends_with(REFUSALS) {
            measures.insert("native_refusals".to_string(), refusals(&source));
        } else if let Ok(syntax) = syn::parse_file(&source) {
            facts.read(&file, &syntax);
        }
    }
    facts.unlowered().for_each(|family| { measures.insert(format!("native_form {family}"), 1); });
}

/// What the catalog and the lane lowerings say about the families.
#[derive(Default)]
struct Facts {
    /// The catalog's `Class` families.
    families: BTreeSet<String>,
    /// The families a lowering names, or the table gives a generic step.
    named: BTreeSet<String>,
    /// The `JsDenotation` variants a lowering names.
    denotated: BTreeSet<String>,
    /// The `Op` variants the allocation decoder names.
    ops: BTreeSet<String>,
    /// The families each denotation names in the table.
    denotations: BTreeMap<String, Vec<String>>,
    /// The family of each opcode of the table.
    classes: BTreeMap<String, String>,
    /// The families whose staged decoder the lane steps.
    decoded: BTreeSet<String>,
}

impl Facts {
    /// Sorts what a parsed file contributes: the catalog's `Class` families, the classes and
    /// denotations its table gives, or the names a lane lowering uses.
    fn read(&mut self, file: &str, syntax: &syn::File) {
        if file.ends_with(CATALOG) {
            self.families.extend(class_variants(syntax));
        } else if file.ends_with(TABLE) {
            self.named.extend(generic_steps(syntax));
            for row in catalog::table(syntax) {
                let Some(class) = row.value(catalog::CLASS) else { continue };
                self.classes.insert(row.name.clone(), class.to_owned());
                if let Some(denotation) = row.value(catalog::JS) { self.denotations.entry(denotation.to_owned()).or_default().push(class.to_owned()); }
                if row.value(catalog::STAGED).is_some_and(|decoder| catalog::LANE_DECODERS.contains(&decoder)) { self.decoded.insert(class.to_owned()); }
            }
        } else if file.ends_with(ALLOCATION) {
            self.ops.extend(path_names(syntax).into_iter().filter(|(owner, _)| owner == "Op").map(|(_, name)| name));
        } else {
            for (owner, name) in path_names(syntax) {
                match owner.as_str() {
                    "Class" => { self.named.insert(name); }
                    "JsDenotation" => { self.denotated.insert(name); }
                    _ => {}
                }
            }
        }
    }

    /// The families with no lowering: neither named, nor given by a denotation a lowering names.
    fn unlowered(&self) -> impl Iterator<Item = &String> {
        let by_denotation: BTreeSet<&String> = self.denotated.iter().filter_map(|name| self.denotations.get(name)).flatten()
            .chain(self.ops.iter().filter_map(|op| self.classes.get(op))).chain(&self.decoded).collect();
        self.families.iter()
            .filter(|family| !EXEMPT.contains(&family.as_str()) && !family.starts_with(EXEMPT_PREFIX))
            .filter(move |family| !self.named.contains(*family) && !by_denotation.contains(family))
    }
}

/// The instructions a JIT file builds by hand (`native_ir <file>`), outside test modules and
/// test paths.
fn hand_built(file: &str, path: &PathBuf, measures: &mut Measures) {
    let jit = file.contains(JIT_DIRECTORY) || file.ends_with(JIT_ROOT);
    if !jit || file.contains(MACHINE_HOME) || crate::in_test_path(file) { return; }
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

/// The `Owner::Name` paths of a file, the owner being the path's second-last segment.
fn path_names(syntax: &syn::File) -> Vec<(String, String)> {
    struct Paths(Vec<(String, String)>);
    impl<'ast> Visit<'ast> for Paths {
        fn visit_path(&mut self, path: &'ast syn::Path) {
            if let [.., owner, name] = path.segments.iter().collect::<Vec<_>>().as_slice() {
                self.0.push((owner.ident.to_string(), name.ident.to_string()));
            }
            syn::visit::visit_path(self, path);
        }
    }
    let mut paths = Paths(Vec::new());
    paths.visit_file(syntax);
    paths.0
}
