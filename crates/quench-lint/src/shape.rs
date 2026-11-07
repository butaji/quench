//! The manifesto's size laws: a function spans at most [`MAX_FUNCTION_LINES`] lines with
//! cyclomatic complexity at most [`MAX_COMPLEXITY`], and a file at most [`MAX_FILE_LINES`].
//!
//! Code written before the laws is listed in `oversized.txt` with its measure. The list only
//! shrinks: an unlisted offender, a listed one that grew, and an entry that no longer
//! offends all fail; `--update` rewrites the list to the current, smaller measures.

use std::{collections::BTreeMap, fmt::Write as _};

use quote::ToTokens;
use syn::{spanned::Spanned, visit::Visit};

pub const MAX_FUNCTION_LINES: usize = 40;
pub const MAX_COMPLEXITY: usize = 10;
pub const MAX_FILE_LINES: usize = 500;

/// What exceeds a law, by `rule subject`, with its measure.
pub type Measures = BTreeMap<String, usize>;

/// Records every file and function that exceeds a law.
pub fn scan(file: &str, source: &str, syntax: &syn::File, measures: &mut Measures) {
    let lines = source.lines().count();
    if lines > MAX_FILE_LINES {
        measures.insert(format!("file-lines {file}"), lines);
    }
    crate::data_layout::scan(file, syntax, measures);
    let hot = crate::hot_path::is_hot(file);
    let mut functions = Functions { file, scope: Vec::new(), measures, seen: BTreeMap::new(), hot, test: false };
    functions.visit_file(syntax);
}

struct Functions<'a> {
    file: &'a str,
    /// Enclosing modules, impl types and functions, naming a function within its file.
    scope: Vec<String>,
    measures: &'a mut Measures,
    /// How often each qualified name has occurred, to tell same-named functions apart.
    seen: BTreeMap<String, usize>,
    /// Whether the hot-path cost model applies: a hot module, outside its tests.
    hot: bool,
    /// Whether the scope exists only for tests.
    test: bool,
}

impl Functions<'_> {
    fn function(&mut self, ident: &syn::Ident, whole: proc_macro2::Span, body: &syn::Block) {
        self.scope.push(ident.to_string());
        let name = self.scope.join("::");
        let occurrence = self.seen.entry(name.clone()).and_modify(|count| *count += 1).or_insert(0);
        let subject = match occurrence {
            0 => format!("{}::{name}", self.file),
            n => format!("{}::{name}#{n}", self.file),
        };
        let lines = whole.end().line - whole.start().line + 1;
        if lines > MAX_FUNCTION_LINES {
            self.measures.insert(format!("function-lines {subject}"), lines);
        }
        let mut decisions = Decisions(0);
        decisions.visit_block(body);
        if decisions.0 + 1 > MAX_COMPLEXITY {
            self.measures.insert(format!("complexity {subject}"), decisions.0 + 1);
        }
        if self.hot {
            crate::hot_path::scan(&subject, body, self.measures);
        }
        if !self.test {
            crate::guest_names::scan(&subject, body, self.measures);
        }
        self.visit_block(body);
        self.scope.pop();
    }

    fn scoped(&mut self, name: String, visit: impl FnOnce(&mut Self)) {
        self.scope.push(name);
        visit(self);
        self.scope.pop();
    }
}

impl<'ast> Visit<'ast> for Functions<'_> {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        let (hot, test) = (self.hot, self.test);
        self.test |= is_test(&item.attrs);
        self.hot &= !self.test;
        self.function(&item.sig.ident, item.span(), &item.block);
        (self.hot, self.test) = (hot, test);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.function(&item.sig.ident, item.span(), &item.block);
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        if let Some(body) = &item.default {
            self.function(&item.sig.ident, item.span(), body);
        }
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        let (hot, test) = (self.hot, self.test);
        self.test |= is_test(&item.attrs);
        self.hot &= !self.test;
        self.scoped(item.ident.to_string(), |this| syn::visit::visit_item_mod(this, item));
        (self.hot, self.test) = (hot, test);
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let name = item.self_ty.to_token_stream().to_string().replace(' ', "");
        self.scoped(name, |this| syn::visit::visit_item_impl(this, item));
    }

    fn visit_item_trait(&mut self, item: &'ast syn::ItemTrait) {
        self.scoped(item.ident.to_string(), |this| syn::visit::visit_item_trait(this, item));
    }
}

/// Whether an item exists only for tests (`#[test]`, `#[cfg(test)]`, `#[cfg(all(test, ..))]`).
pub fn is_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        let text = attr.to_token_stream().to_string();
        text == "# [test]" || text.starts_with("# [cfg (test") || text.starts_with("# [cfg (all (test")
    })
}

/// Counts a body's decision points; a nested function is its own body.
struct Decisions(usize);

impl<'ast> Visit<'ast> for Decisions {
    fn visit_expr(&mut self, expr: &'ast syn::Expr) {
        self.0 += match expr {
            syn::Expr::If(_) | syn::Expr::While(_) | syn::Expr::ForLoop(_) | syn::Expr::Loop(_) => 1,
            syn::Expr::Match(arms) => arms.arms.len().saturating_sub(1),
            syn::Expr::Binary(binary) => usize::from(matches!(binary.op, syn::BinOp::And(_) | syn::BinOp::Or(_))),
            _ => 0,
        };
        syn::visit::visit_expr(self, expr);
    }

    fn visit_item(&mut self, _: &'ast syn::Item) {}
}

/// The recorded offenders, as `oversized.txt` spells them.
pub fn parse(listing: &str) -> Measures {
    listing
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.rsplit_once(' '))
        .filter_map(|(subject, measure)| Some((subject.to_owned(), measure.parse().ok()?)))
        .collect()
}

pub fn render(measures: &Measures) -> String {
    let mut listing = String::from("# Generated by `quench-lint --update`; entries may only shrink or disappear.\n");
    for (subject, measure) in measures {
        let _ = writeln!(listing, "{subject} {measure}");
    }
    listing
}

/// How to bring a new offender within its law.
fn remedy(subject: &str) -> &'static str {
    if subject.starts_with("guest-name ") {
        "no special case for one program: derive the behavior from semantics, never from a guest name"
    } else if subject.starts_with("dod-bool-array ") {
        "flags belong in a bitset: one bit per flag, scanned a word at a time"
    } else if subject.starts_with("dod-dense-hash ") {
        "a dense id indexes a flat array; pack frequent flags into the index's high bits"
    } else if subject.starts_with("dod-cache ") {
        "a cache hides a slow core path: make the core path cheap and delete the cache"
    } else if subject.starts_with("hot-") {
        "hot paths do O(1) work and allocate nothing: decide earlier, index a flat array, reuse a buffer"
    } else {
        "split it along a domain seam"
    }
}

/// Prints each departure from the recorded list; returns how many there are.
pub fn compare(recorded: &Measures, current: &Measures) -> usize {
    let mut departures = 0;
    for (subject, &measure) in current {
        match recorded.get(subject) {
            None => println!("oversized: {subject} is {measure}; {}", remedy(subject)),
            Some(&limit) if measure > limit => println!("oversized: {subject} grew from {limit} to {measure}"),
            Some(_) => continue,
        }
        departures += 1;
    }
    for subject in recorded.keys().filter(|subject| !current.contains_key(*subject)) {
        println!("oversized: {subject} now complies; run `quench-lint --update` to drop it");
        departures += 1;
    }
    departures
}
