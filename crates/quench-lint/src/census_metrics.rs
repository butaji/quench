//! The census's extraction functions: each reads one parsed source file and counts one thing.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use proc_macro2::{TokenStream, TokenTree};
use syn::{Item, visit::Visit};

use crate::catalog::{self, is_punct};
use crate::shape::is_test;

const OPCODE_OWNERS: &[&str] = &["Op", "Class"];
const OPCODE_CATALOG: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../quench-runtime/src/bytecode/op_catalog.rs"));
const PREDICATE_TABLE_MACRO: &str = "op_predicates";
const OPCODE_RECEIVERS: &[&str] = &["op", "opcode"];
const OPCODE_ACCESSOR: &str = "op";
const CLASS_OWNER: &[&str] = &["Class"];
const DECODER_ROW_FUNCTIONS: &[&str] = &["numeric_row", "heap_row", "named_row"];
const LANE_EXIT: &str = "LaneExit";
const TIER_TABLE: &str = "MEASURED";
const ENTRY_ALIAS: &str = "Entry";
const EXIT_SUFFIXES: &[&str] = &["_EXIT", "_STATUS"];
const FRAME_NAMES: &[&str] = &["Frame", "Activation"];
const FRAME_SUFFIX: &str = "Frame";
const COMMENT_PREFIX: &str = "#";
const UNTAGGED_FLAGS: [&str; 2] = ["MemFlagsData", "trusted"];
const FACT_HOME_SUFFIXES: &[&str] = &["Memo", "Memos", "Cache", "Hint", "Hints", "Proof", "Proofs"];
const EPOCH_SUFFIX: &str = "epoch";
const GUEST_REENTRY: &str = "call_value";
const WALL_CLOCK: [&str; 2] = ["Instant", "now"];
const STABLE_EPOCH_READS: [&str; 2] = ["prototype_layout_epoch", "intrinsic_epoch"];

/// The items of a module tree, nested inline modules flattened, test modules left out.
pub fn module_items(items: &[Item]) -> Vec<&Item> {
    items
        .iter()
        .flat_map(|item| match item {
            Item::Mod(module) if is_test(&module.attrs) => Vec::new(),
            Item::Mod(module) => module.content.iter().flat_map(|(_, inner)| module_items(inner)).collect(),
            other => vec![other],
        })
        .collect()
}

/// The variant a path names when it is `Owner::Variant` for one of `owners`.
fn owned_variant(path: &syn::Path, owners: &[&str]) -> Option<String> {
    let mut segments = path.segments.iter().rev();
    let variant = segments.next()?;
    let owner = segments.next()?;
    owners.iter().any(|name| owner.ident == name).then(|| variant.ident.to_string())
}

/// Match arms whose pattern names a variant of `owners`, and the distinct variants named.
struct Arms<'a> {
    owners: &'a [&'a str],
    arms: usize,
    variants: BTreeSet<String>,
}

impl<'a> Arms<'a> {
    fn new(owners: &'a [&'a str]) -> Self {
        Self { owners, arms: 0, variants: BTreeSet::new() }
    }
}

struct PatternPaths<'a>(&'a [&'a str], Vec<String>);

impl<'ast> Visit<'ast> for PatternPaths<'_> {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        self.1.extend(owned_variant(path, self.0));
    }
}

impl<'ast> Visit<'ast> for Arms<'_> {
    fn visit_arm(&mut self, arm: &'ast syn::Arm) {
        let mut named = PatternPaths(self.owners, Vec::new());
        named.visit_pat(&arm.pat);
        self.arms += usize::from(!named.1.is_empty());
        self.variants.extend(named.1);
        syn::visit::visit_arm(self, arm);
    }

    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if !is_test(&module.attrs) {
            syn::visit::visit_item_mod(self, module);
        }
    }
}

/// The opcodes the `opcodes!` table declares: one row each.
pub fn opcode_variants(file: &syn::File) -> usize {
    catalog::table(file).len()
}

/// The opcodes the catalog gives a staged operation: the rows that declare a `staged` decoder.
pub fn staged_opcodes(file: &syn::File) -> usize {
    catalog::table(file).iter().filter(|row| row.declares(catalog::STAGED)).count()
}

/// Non-empty lines that are not `#` comments.
/// Guards a staged row takes in its own body (`.guard(` and `choose_leaving(` calls): each re-checks what a carried
/// fact (`staged::proof`) could prove once (TOD-2417).
pub fn staged_rechecks(text: &str) -> usize {
    text.lines().filter(|line| !line.trim_start().starts_with("//"))
        .map(|line| line.matches(".guard(").count() + line.matches("choose_leaving(").count()).sum()
}

/// Places the u-jit lowers by hand (`object_slots::emit(` stages and `emit_js_guard(` tests it builds itself) instead
/// of running a staged row: each is a second lowering of a semantics the staged rows define once (TOD-2417).
pub fn hand_lowerings(text: &str) -> usize {
    text.lines().filter(|line| !line.trim_start().starts_with("//"))
        .map(|line| line.matches("object_slots::emit(").count() + line.matches("emit_js_guard(").count()).sum()
}

fn code_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines().filter(|line| !line.trim_start().starts_with("//"))
}

/// Second rows for one fact: staged functions named for the fact they assume (`_within`, `_proved`, `_known`).
pub fn variant_rows(text: &str) -> usize {
    const SUFFIXES: [&str; 3] = ["_within", "_proved", "_known"];
    code_lines(text).filter_map(|line| line.split("fn ").nth(1)?.split(['<', '(']).next())
        .filter(|name| SUFFIXES.iter().any(|suffix| name.ends_with(suffix))).count()
}

/// Calls from staged rows into helpers no tier specializes.
pub fn opaque_helper_calls(text: &str) -> usize {
    const OPAQUE: [&str; 4] = [".vm_step(", ".call_out(", ".call_helper(", ".call_native("];
    code_lines(text).map(|line| OPAQUE.iter().map(|call| line.matches(call).count()).sum::<usize>()).sum()
}

/// Hand-written raw memory lines in the stencils, Wasm and RegExp included (both still lack a staged form).
pub fn bjit_stencil_lines(text: &str) -> usize {
    const RAW: [&str; 8] = ["read_volatile", "write_volatile", ".read()", ".write(", "ptr::read", "ptr::write", "as *const", "as *mut"];
    code_lines(text).filter(|line| RAW.iter().any(|raw| line.contains(raw))).count()
}

pub fn text_lines(text: &str) -> usize {
    text.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with(COMMENT_PREFIX)).count()
}

/// The counts of a `... token count` debt file (`staged/derivation_debt.txt`, `staged/fixpoint_debt.txt`):
/// the last field of each line that is not a comment, summed, over the lines `keep` admits.
fn debt_counts(text: &str, keep: impl Fn(&str) -> bool) -> usize {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with(COMMENT_PREFIX) && keep(line))
        .filter_map(|line| line.rsplit_once(' ')?.1.parse::<usize>().ok())
        .sum()
}

/// Every count of a `file count` or `file token count` debt file.
pub fn debt_total(text: &str) -> usize {
    debt_counts(text, |_| true)
}

/// The opcode and family names of `staged/derivation_debt.txt`: the counts of its `op:X`, `Class:X` and
/// `JsDenotation:X` tokens, a tier file that names an opcode or an opcode family outside its staged row.
pub fn opcode_name_debt(text: &str) -> usize {
    debt_counts(text, |line| {
        line.split(' ').nth(1).is_some_and(|token| ["op:", "Class:", "JsDenotation:"].iter().any(|tag| token.starts_with(tag)))
    })
}

/// The listed fallbacks of one kind (`crates/quench-tier-corpus/fallbacks.txt`, lines `kind operation (reason)`).
fn fallbacks_of(text: &str, kind: &str) -> usize {
    text.lines().filter(|line| line.split_once(' ').is_some_and(|(first, _)| first == kind)).count()
}

pub fn fallback_jit_rejects(text: &str) -> usize { fallbacks_of(text, "jit-reject") }
pub fn fallback_helper_calls(text: &str) -> usize { fallbacks_of(text, "helper-call") }
pub fn fallback_deopts(text: &str) -> usize { fallbacks_of(text, "deopt") }
pub fn fallback_opaque_natives(text: &str) -> usize { fallbacks_of(text, "opaque-native") }

/// What the tier crates still take of the runtime's internals: Value words made from raw bits, outside
/// comments, and the heap names `tier_surface` re-exports. The heap module and `Vm::heap` are
/// `pub(crate)` (S1.4), so paths into the heap and reads of the heap field cannot be written; this
/// counter carries the raw-word typing of S1.5 (TOD-2076) and is deleted when it reaches the surface
/// the tiers keep.
pub fn runtime_internals(text: &str) -> usize {
    const REACHES: [&str; 1] = ["Value::from_bits("];
    let reaches: usize = text.lines().map(str::trim).filter(|line| !line.starts_with("//"))
        .map(|line| REACHES.iter().map(|reach| line.matches(reach).count()).sum::<usize>()).sum();
    reaches + surface_names(text)
}

/// The heap items a `pub use crate::heap::...;` re-exports (`quench_runtime::tier_surface`): each is an
/// internal the tiers can still name, so the counter falls only when an item leaves the surface.
fn surface_names(text: &str) -> usize {
    let code: String = text.lines().filter(|line| !line.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
    code.match_indices("pub use crate::heap::").map(|(at, _)| {
        let statement = &code[at..code[at..].find(';').map_or(code.len(), |end| at + end)];
        match (statement.find('{'), statement.rfind('}')) {
            (Some(open), Some(close)) => statement[open + 1..close].split(',').filter(|name| !name.trim().is_empty()).count(),
            _ => 1,
        }
    }).sum()
}

/// The opcode-identity predicates the catalog generates (`is_load_capture`, ...): the left of each
/// `name => Opcode | ...` entry of its `op_predicates!` table.
fn catalog_predicates() -> &'static BTreeSet<String> {
    static NAMES: OnceLock<BTreeSet<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let catalog = syn::parse_file(OPCODE_CATALOG).expect("the opcode catalog parses");
        let tables = catalog.items.iter().filter_map(|item| match item {
            Item::Macro(call) if call.mac.path.is_ident(PREDICATE_TABLE_MACRO) => Some(&call.mac.tokens),
            _ => None,
        });
        tables.flat_map(predicate_names).collect()
    })
}

/// The identifiers an `op_predicates!` table names before a `=>`.
fn predicate_names(tokens: &TokenStream) -> Vec<String> {
    let trees: Vec<TokenTree> = tokens.clone().into_iter().collect();
    (0..trees.len())
        .filter(|&at| is_punct(trees.get(at + 1), '=') && (at == 0 || is_punct(trees.get(at - 1), ',')))
        .filter_map(|at| match &trees[at] {
            TokenTree::Ident(name) => Some(name.to_string()),
            _ => None,
        })
        .collect()
}

/// Whether `receiver` is an opcode: a variable named like one, or an instruction's `.op()`.
fn opcode_receiver(receiver: &syn::Expr) -> bool {
    match receiver {
        syn::Expr::Path(path) => OPCODE_RECEIVERS.iter().any(|name| path.path.is_ident(name)),
        syn::Expr::MethodCall(call) => call.args.is_empty() && call.method == OPCODE_ACCESSOR,
        _ => false,
    }
}

/// Calls of the catalog's opcode-identity predicates on an opcode (`opcode.is_load_capture()`,
/// `instruction.op().is_return()`): a branch on one opcode's identity written as a call, which
/// a renamed arm must not hide.
struct Predicates {
    calls: usize,
}

impl<'ast> Visit<'ast> for Predicates {
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        self.calls += usize::from(
            call.args.is_empty() && catalog_predicates().contains(&call.method.to_string()) && opcode_receiver(&call.receiver),
        );
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if !is_test(&module.attrs) {
            syn::visit::visit_item_mod(self, module);
        }
    }
}

/// Match arms that decide by opcode or opcode class, and calls of the catalog's opcode-identity
/// predicates (`is_<opcode>()`), which decide by one opcode without naming it as a pattern.
pub fn opcode_arms(file: &syn::File) -> usize {
    let mut arms = Arms::new(OPCODE_OWNERS);
    arms.visit_file(file);
    let mut predicates = Predicates { calls: 0 };
    predicates.visit_file(file);
    arms.arms + predicates.calls
}

/// Structs named like a frame or an activation.
pub fn frame_structs(file: &syn::File) -> usize {
    module_items(&file.items)
        .into_iter()
        .filter(|item| matches!(item, Item::Struct(found) if {
            let name = found.ident.to_string();
            name.ends_with(FRAME_SUFFIX) || FRAME_NAMES.contains(&name.as_str())
        }))
        .count()
}

/// `LaneExit` variants and module-level `_EXIT` / `_STATUS` constants.
pub fn exit_mechanisms(file: &syn::File) -> usize {
    module_items(&file.items)
        .into_iter()
        .map(|item| match item {
            Item::Enum(found) if found.ident == LANE_EXIT => found.variants.len(),
            Item::Const(found) => usize::from(EXIT_SUFFIXES.iter().any(|suffix| found.ident.to_string().ends_with(suffix))),
            _ => 0,
        })
        .sum()
}

struct Numbers(usize);

impl<'ast> Visit<'ast> for Numbers {
    fn visit_lit(&mut self, literal: &'ast syn::Lit) {
        self.0 += usize::from(matches!(literal, syn::Lit::Int(_) | syn::Lit::Float(_)));
    }
}

/// Numeric literals in the initializer of the `MEASURED` constant.
pub fn tier_policy_literals(file: &syn::File) -> usize {
    let mut numbers = Numbers(0);
    for item in module_items(&file.items) {
        if let Item::Const(table) = item
            && table.ident == TIER_TABLE
        {
            numbers.visit_expr(&table.expr);
        }
    }
    numbers.0
}

struct EntryUses(usize);

impl<'ast> Visit<'ast> for EntryUses {
    fn visit_type_path(&mut self, path: &'ast syn::TypePath) {
        self.0 += usize::from(path.qself.is_none() && path.path.is_ident(ENTRY_ALIAS));
        syn::visit::visit_type_path(self, path);
    }
}

/// References to the `Entry` alias; a file that defines its own `Entry` type is not one.
pub fn entry_uses(file: &syn::File) -> usize {
    let own_type = module_items(&file.items).into_iter().any(|item| {
        matches!(item, Item::Struct(found) if found.ident == ENTRY_ALIAS)
            || matches!(item, Item::Enum(found) if found.ident == ENTRY_ALIAS)
    });
    let mut uses = EntryUses(0);
    if !own_type {
        uses.visit_file(file);
    }
    uses.0
}

/// The distinct opcode classes the b-jit decoder turns into a row: the `Class::X` arms of its
/// row functions (an arm that only rejects is not a row).
pub fn decoder_classes(file: &syn::File) -> usize {
    let mut arms = Arms::new(CLASS_OWNER);
    for item in module_items(&file.items) {
        if let Item::Fn(found) = item
            && DECODER_ROW_FUNCTIONS.iter().any(|name| found.sig.ident == name)
        {
            arms.visit_item_fn(found);
        }
    }
    arms.variants.len()
}

struct UntaggedFlags(usize);

impl<'ast> Visit<'ast> for UntaggedFlags {
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(callee) = &*call.func {
            let names: Vec<String> = callee.path.segments.iter().map(|segment| segment.ident.to_string()).collect();
            self.0 += usize::from(names.ends_with(&UNTAGGED_FLAGS.map(String::from)));
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if !is_test(&module.attrs) {
            syn::visit::visit_item_mod(self, module);
        }
    }
}

/// Native memory accesses built from `MemFlagsData::trusted()`, which name no alias region
/// (the tagged ones go through `vm/compiled/regions.rs`), outside test modules.
pub fn untagged_memory_accesses(file: &syn::File) -> usize {
    let mut found = UntaggedFlags(0);
    found.visit_file(file);
    found.0
}

/// Types whose name says they hold a private copy of a fact (memo, cache, hint, proof): each one
/// is a home the one fact store replaces (principle P1).
pub fn fact_homes(file: &syn::File) -> usize {
    module_items(&file.items)
        .into_iter()
        .filter(|item| matches!(item, Item::Struct(found) if {
            let name = found.ident.to_string();
            FACT_HOME_SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
        }))
        .count()
}

/// Struct fields named `*epoch`: each is a validity counter a reader compares per use, which a
/// code dependency replaces for stable facts (P1).
pub fn epoch_fields(file: &syn::File) -> usize {
    module_items(&file.items)
        .into_iter()
        .map(|item| match item {
            Item::Struct(found) => found
                .fields
                .iter()
                .filter(|field| field.ident.as_ref().is_some_and(|name| name.to_string().ends_with(EPOCH_SUFFIX)))
                .count(),
            _ => 0,
        })
        .sum()
}

/// Calls that match a predicate, outside test modules.
struct Calls<F>(F, usize);

impl<'ast, F: FnMut(&'ast syn::Expr) -> bool> Visit<'ast> for Calls<F> {
    fn visit_expr(&mut self, expr: &'ast syn::Expr) {
        self.1 += usize::from((self.0)(expr));
        syn::visit::visit_expr(self, expr);
    }

    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if !is_test(&module.attrs) {
            syn::visit::visit_item_mod(self, module);
        }
    }
}

fn count_calls<'ast>(file: &'ast syn::File, matches: impl FnMut(&'ast syn::Expr) -> bool) -> usize {
    let mut calls = Calls(matches, 0);
    calls.visit_file(file);
    calls.1
}

/// `.call_value(..)` calls: Rust re-entering guest code through a nested run instead of
/// returning a continuation to the lane (P2).
pub fn guest_reentries(file: &syn::File) -> usize {
    count_calls(file, |expr| matches!(expr, syn::Expr::MethodCall(call) if call.method == GUEST_REENTRY))
}

/// `Instant::now()` calls: policy read from wall time instead of the work clock (P7).
pub fn wall_clock_reads(file: &syn::File) -> usize {
    count_calls(file, |expr| {
        matches!(expr, syn::Expr::Call(call) if matches!(&*call.func, syn::Expr::Path(callee)
            if callee.path.segments.iter().rev().take(WALL_CLOCK.len()).map(|segment| segment.ident.to_string())
                .eq(WALL_CLOCK.iter().rev().map(|name| name.to_string()))))
    })
}

/// Reads of a stable-fact epoch: a per-use re-check that a code dependency replaces (P1).
pub fn stable_epoch_reads(file: &syn::File) -> usize {
    count_calls(file, |expr| match expr {
        syn::Expr::MethodCall(call) => STABLE_EPOCH_READS.iter().any(|name| call.method == *name),
        syn::Expr::Call(call) => match &*call.func {
            syn::Expr::Path(callee) => {
                let last = callee.path.segments.last();
                last.is_some_and(|last| STABLE_EPOCH_READS.iter().any(|name| last.ident == *name))
            }
            _ => false,
        },
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> syn::File {
        syn::parse_str(source).expect("test source parses")
    }

    #[test]
    fn opcode_table_entries_are_counted() {
        let file = parse("opcodes!($; Nop => Effect::PURE; layout Scalar, @ R, Add => X::Y, @ R);");
        assert_eq!(opcode_variants(&file), 2);
    }

    #[test]
    fn staged_opcodes_are_the_rows_that_declare_a_decoder() {
        let file = parse(
            "opcodes!($; Nop => Effect::PURE; layout Scalar, @ R, Add => X::Y; layout Scalar; staged Flow; step Body, @ R,
             Sub => X::Y; layout Scalar; staged Value, @ R);",
        );
        assert_eq!(staged_opcodes(&file), 2);
    }

    #[test]
    fn text_lines_skip_comments_and_blanks() {
        assert_eq!(text_lines("# note\n\nfoo Op::A 1\n  bar 2\n"), 2);
    }

    #[test]
    fn debt_files_sum_their_counts() {
        let derivation = "# total 9\na.rs fn:lane_* 4\na.rs op:Add 2\nb.rs Class:Call 1\nc.rs list:Nop 2\n";
        assert_eq!(debt_total(derivation), 9);
        assert_eq!(opcode_name_debt(derivation), 3);
        assert_eq!(debt_total("# total 3\nquench-runtime/src/webs.rs 1\nx/y.rs 2\n"), 3);
    }

    #[test]
    fn opcode_arms_count_op_and_class_patterns_outside_tests() {
        let file = parse(
            "fn f() { match x { Op::A => 1, Class::B | Class::C => 2, Other::D => 3, _ => 4 } }
             #[cfg(test)] mod tests { fn g() { match x { Op::A => 1, _ => 2 } } }",
        );
        assert_eq!(opcode_arms(&file), 2);
    }

    #[test]
    fn opcode_arms_count_identity_predicates_on_opcodes_outside_tests() {
        let file = parse(
            "fn f(i: I) { match i.op() { opcode if opcode.is_return() && x.is_nop() => 1, _ if i.op().is_load_capture() => 2,
                 _ if cell.is_return() => 3, _ => 4 } }
             fn g(op: Op) -> bool { op.is_jump() || op.allocates() }
             #[cfg(test)] mod tests { fn h(op: Op) -> bool { op.is_jump() } }",
        );
        assert_eq!(opcode_arms(&file), 3);
    }

    #[test]
    fn the_catalog_names_its_identity_predicates() {
        let names = catalog_predicates();
        assert!(names.contains("is_load_capture") && names.contains("is_jump_binary_false"));
        assert!(!names.contains("allocates"));
    }

    #[test]
    fn frame_structs_match_names() {
        let file = parse("struct Frame; struct LaneFrame; struct Activation; struct Framer;");
        assert_eq!(frame_structs(&file), 3);
    }

    #[test]
    fn exit_mechanisms_count_variants_and_constants() {
        let file = parse("enum LaneExit { A, B, C } const X_EXIT: u8 = 0; const Y_STATUS: u8 = 1; const Z: u8 = 2;");
        assert_eq!(exit_mechanisms(&file), 5);
    }

    #[test]
    fn tier_policy_literals_count_numbers_in_measured() {
        let file = parse("const OTHER: T = T { a: 7 }; const MEASURED: T = T { a: 1000, b: (1, 2), c: -16_000, e: usize::MAX, ..MEASURED.x };");
        assert_eq!(tier_policy_literals(&file), 4);
    }

    #[test]
    fn entry_uses_are_type_references_outside_the_definition() {
        let file = parse("type Entry = fn(); struct S { e: Entry } fn f(a: Option<Entry>, b: LaneExit::Entry) {}");
        assert_eq!(entry_uses(&file), 2);
        assert_eq!(entry_uses(&parse("struct Entry; fn f(a: Entry) {}")), 0);
    }

    #[test]
    fn decoder_classes_are_distinct_arms_of_the_row_functions() {
        let file = parse(
            "fn numeric_row(i: I) { match i.class() { Class::A | Class::B => 1, _ => 2 } }
             fn heap_row(i: I) { match i.class() { Class::B => 1, Class::C if x => 2, _ => 3 } }
             fn named_row(i: I) { match i.class() { Class::E => 1, _ => 2 } }
             fn reject(i: I) { match i.class() { Class::D => 1, _ => 2 } }",
        );
        assert_eq!(decoder_classes(&file), 4);
    }

    #[test]
    fn untagged_accesses_are_trusted_flags_calls_outside_tests() {
        let file = parse(
            "fn f() { b.load(t, MemFlagsData::trusted(), a, 0); b.store(ir::MemFlagsData::trusted(), v, a, 0); regions::frame(); }
             #[cfg(test)] mod tests { fn g() { MemFlagsData::trusted(); } }",
        );
        assert_eq!(untagged_memory_accesses(&file), 2);
    }

    #[test]
    fn fact_homes_count_memo_cache_hint_and_proof_types() {
        let file = parse("struct ConcatMemo; struct FieldCache; struct HolderHint; struct LocalProofs; struct Cached; mod m { struct ReadMemo; }");
        assert_eq!(fact_homes(&file), 5);
    }

    #[test]
    fn epoch_fields_count_named_fields() {
        let file = parse("struct A { layout_epoch: u32, epoch: u32, epochs: u8 } struct B(u32);");
        assert_eq!(epoch_fields(&file), 2);
    }

    #[test]
    fn guest_reentries_and_wall_clock_reads_skip_tests() {
        let file = parse(
            "fn f() { vm.call_value(a); x.call_values(); let t = Instant::now(); let u = std::time::Instant::now(); now(); }
             #[cfg(test)] mod tests { fn g() { vm.call_value(a); Instant::now(); } }",
        );
        assert_eq!(guest_reentries(&file), 1);
        assert_eq!(wall_clock_reads(&file), 2);
    }

    #[test]
    fn stable_epoch_reads_count_method_and_path_calls_outside_tests() {
        let file = parse(
            "fn prototype_layout_epoch() {}
             fn f() { self.heap.prototype_layout_epoch(); crate::heap::cell::intrinsic_epoch(); prototype_layout_epoch(); x.intrinsic_epochs(); }
             #[cfg(test)] mod tests { fn g() { x.prototype_layout_epoch(); intrinsic_epoch(); } }",
        );
        assert_eq!(stable_epoch_reads(&file), 3);
    }
}
