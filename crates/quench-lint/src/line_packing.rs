//! `line-packing`: statements or functions packed onto one long line, counted per file outside
//! tests. The function-length law counts lines, so moving code onto fewer, longer lines would
//! evade it; a line longer than [`MAX_LINE_CHARS`] that begins two or more statements or
//! functions (`fn f() { a(); b() }`, `let x = 1; let y = 2;`) is counted once.
//!
//! The counts join the shrink-only measures in `oversized.txt`: a new packed line fails, an
//! old one may only be split.

use syn::{spanned::Spanned, visit::Visit};

use crate::shape::{Measures, is_test};

/// The measure packed lines are recorded under, by file.
pub const MEASURE: &str = "line-packing";
/// The width rustfmt wraps at: a longer line that also holds several statements is packed.
pub const MAX_LINE_CHARS: usize = 100;
/// Statements and functions that make a line packed once it is long.
const MIN_BEGUN: usize = 2;

/// Records, by `line-packing <file>`, how many long lines begin several statements or functions.
pub fn scan(file: &str, source: &str, syntax: &syn::File, measures: &mut Measures) {
    if crate::in_test_path(file) {
        return;
    }
    let mut begun = Begun::default();
    begun.visit_file(syntax);
    begun.0.sort_unstable();
    let lines: Vec<&str> = source.lines().collect();
    let packed = begun
        .0
        .chunk_by(|first, second| first == second)
        .filter(|line| line.len() >= MIN_BEGUN && lines.get(line[0] - 1).is_some_and(|text| text.chars().count() > MAX_LINE_CHARS))
        .count();
    if packed > 0 {
        measures.insert(format!("{MEASURE} {file}"), packed);
    }
}

/// The line (1-based) each statement and function begins on.
#[derive(Default)]
struct Begun(Vec<usize>);

impl Begun {
    fn begin(&mut self, node: &impl Spanned) {
        self.0.push(node.span().start().line);
    }
}

impl<'ast> Visit<'ast> for Begun {
    fn visit_stmt(&mut self, stmt: &'ast syn::Stmt) {
        if !matches!(stmt, syn::Stmt::Item(_)) {
            self.begin(stmt);
        }
        syn::visit::visit_stmt(self, stmt);
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if !is_test(&item.attrs) {
            self.begin(item);
            syn::visit::visit_item_fn(self, item);
        }
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.begin(item);
        syn::visit::visit_impl_item_fn(self, item);
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        self.begin(item);
        syn::visit::visit_trait_item_fn(self, item);
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if !is_test(&item.attrs) {
            syn::visit::visit_item_mod(self, item);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packed(source: &str) -> usize {
        let mut measures = Measures::new();
        scan("a.rs", source, &syn::parse_file(source).expect("test source parses"), &mut measures);
        measures.get("line-packing a.rs").copied().unwrap_or(0)
    }

    #[test]
    fn a_long_line_beginning_several_statements_is_packed() {
        let tail = "x".repeat(MAX_LINE_CHARS);
        assert_eq!(packed(&format!("fn f() {{ let a = \"{tail}\"; let b = a; }}\n")), 1);
        assert_eq!(packed(&format!("fn f() {{\n    let a = \"{tail}\"; let b = a;\n}}\n")), 1);
    }

    #[test]
    fn short_lines_and_one_statement_per_line_are_not_packed() {
        let tail = "x".repeat(MAX_LINE_CHARS);
        assert_eq!(packed("fn f() { let a = 1; let b = a; }\n"), 0);
        assert_eq!(packed(&format!("fn f() {{\n    let a = \"{tail}\";\n    let b = a;\n}}\n")), 0);
    }

    #[test]
    fn test_modules_are_left_out() {
        let tail = "x".repeat(MAX_LINE_CHARS);
        assert_eq!(packed(&format!("#[cfg(test)]\nmod tests {{ fn f() {{ let a = \"{tail}\"; let b = a; }} }}\n")), 0);
    }
}
