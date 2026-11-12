//! The census's extraction functions: each reads one parsed source file and counts one thing.

use std::collections::BTreeSet;

use proc_macro2::{TokenStream, TokenTree};
use syn::{Item, visit::Visit};

use crate::shape::is_test;

const OPCODE_OWNERS: &[&str] = &["Op", "Class"];
const BYTECODE_OWNER: &[&str] = &["Op"];
const CLASS_OWNER: &[&str] = &["Class"];
const DECODER_ROW_FUNCTIONS: &[&str] = &["numeric_row", "heap_row"];
const OPCODE_TABLE_MACRO: &str = "opcodes";
const STAGED_OPERATION: &str = "Operation";
const LANE_EXIT: &str = "LaneExit";
const TIER_TABLE: &str = "MEASURED";
const ENTRY_ALIAS: &str = "Entry";
const EXIT_SUFFIXES: &[&str] = &["_EXIT", "_STATUS"];
const FRAME_NAMES: &[&str] = &["Frame", "Activation", "ShadowFrame"];
const FRAME_SUFFIX: &str = "Frame";
const COMMENT_PREFIX: &str = "#";

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

/// The opcodes the `opcodes!` table declares: each entry is `Name => ...` after a separator.
pub fn opcode_variants(file: &syn::File) -> usize {
    let tables = file.items.iter().filter_map(|item| match item {
        Item::Macro(call) if call.mac.path.is_ident(OPCODE_TABLE_MACRO) => Some(&call.mac.tokens),
        _ => None,
    });
    tables.map(entry_names).sum()
}

fn entry_names(tokens: &TokenStream) -> usize {
    let trees: Vec<TokenTree> = tokens.clone().into_iter().collect();
    let is_punct = |tree: Option<&TokenTree>, mark: char| matches!(tree, Some(TokenTree::Punct(punct)) if punct.as_char() == mark);
    (0..trees.len())
        .filter(|&at| {
            let starts = at == 0 || is_punct(trees.get(at - 1), ',') || is_punct(trees.get(at - 1), ';');
            starts && matches!(trees[at], TokenTree::Ident(_)) && is_punct(trees.get(at + 1), '=')
        })
        .count()
}

/// The distinct opcodes on the left of an arm in `impl Operation`.
pub fn staged_opcodes(file: &syn::File) -> usize {
    let mut arms = Arms::new(BYTECODE_OWNER);
    for item in module_items(&file.items) {
        if let Item::Impl(block) = item
            && matches!(&*block.self_ty, syn::Type::Path(path) if path.path.segments.last().is_some_and(|last| last.ident == STAGED_OPERATION))
        {
            arms.visit_item_impl(block);
        }
    }
    arms.variants.len()
}

/// Non-empty lines that are not `#` comments.
pub fn text_lines(text: &str) -> usize {
    text.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with(COMMENT_PREFIX)).count()
}

/// Match arms that decide by opcode or opcode class.
pub fn opcode_arms(file: &syn::File) -> usize {
    let mut arms = Arms::new(OPCODE_OWNERS);
    arms.visit_file(file);
    arms.arms
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
    fn staged_opcodes_are_distinct_arms_of_operation_impls() {
        let file = parse(
            "impl<W> Operation<W> { fn a(i: I) { match i.op() { Op::A | Op::B => 1, Op::A => 2, _ => 3 } } }
             impl Other { fn b(i: I) { match i { Op::C => 1, _ => 2 } } }",
        );
        assert_eq!(staged_opcodes(&file), 2);
    }

    #[test]
    fn text_lines_skip_comments_and_blanks() {
        assert_eq!(text_lines("# note\n\nfoo Op::A 1\n  bar 2\n"), 2);
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
    fn frame_structs_match_names() {
        let file = parse("struct Frame; struct LaneFrame; struct Activation; struct Framer; mod m { struct ShadowFrame; }");
        assert_eq!(frame_structs(&file), 4);
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
             fn reject(i: I) { match i.class() { Class::D => 1, _ => 2 } }",
        );
        assert_eq!(decoder_classes(&file), 3);
    }
}
