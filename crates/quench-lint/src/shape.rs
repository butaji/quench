//! The manifesto's size laws: a function spans at most [`MAX_FUNCTION_LINES`] lines with
//! cyclomatic complexity at most [`MAX_COMPLEXITY`], and a file at most [`MAX_FILE_LINES`].
//!
//! Code written before the laws is listed in `oversized.txt` with its measure. The list only
//! shrinks: an unlisted offender, a listed one that grew, and an entry that no longer
//! offends all fail; `--update` rewrites the list to the current, smaller measures, and
//! `--lower` rewrites only the entries that shrank or no longer offend (never one that grew,
//! never a new one).

use std::{collections::BTreeMap, fmt::Write as _};

use quote::ToTokens;
use syn::{spanned::Spanned, visit::Visit};

use crate::decision_trees::DecisionTrees;

pub const MAX_FUNCTION_LINES: usize = 40;
pub const MAX_COMPLEXITY: usize = 10;
pub const MAX_FILE_LINES: usize = 500;

/// What exceeds a law, by `rule subject`, with its measure.
pub type Measures = BTreeMap<String, usize>;

/// Records every file and function that exceeds a law.
pub fn scan(file: &str, source: &str, syntax: &syn::File, measures: &mut Measures, trees: &mut DecisionTrees) {
    let lines = source.lines().count();
    if lines > MAX_FILE_LINES {
        measures.insert(format!("file-lines {file}"), lines);
    }
    crate::data_layout::scan(file, syntax, measures);
    crate::fn_identity::scan(file, syntax, measures);
    let hot = crate::hot_path::is_hot(file) && !crate::test_paths::in_test_path(file);
    let mut functions = Functions { file, scope: Vec::new(), measures, trees, seen: BTreeMap::new(), hot, test: false };
    functions.visit_file(syntax);
}

struct Functions<'a> {
    file: &'a str,
    /// Enclosing modules, impl types and functions, naming a function within its file.
    scope: Vec<String>,
    measures: &'a mut Measures,
    /// Case analyses across the repository, to find one decided twice.
    trees: &'a mut DecisionTrees,
    /// How often each qualified name has occurred, to tell same-named functions apart.
    seen: BTreeMap<String, usize>,
    /// Whether the hot-path cost model applies: a hot module, outside its tests.
    hot: bool,
    /// Whether the scope exists only for tests.
    test: bool,
}

impl Functions<'_> {
    fn function(&mut self, ident: &syn::Ident, attrs: &[syn::Attribute], whole: proc_macro2::Span, body: &syn::Block) {
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
            crate::fast_paths::scan(&subject, lines, attrs, body, self.measures);
            crate::guest_names::scan(&subject, body, self.measures);
            crate::fingerprints::scan(&subject, body, self.measures);
            crate::magic::scan(&subject, body, self.measures);
            self.trees.scan(&subject, body);
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
        self.function(&item.sig.ident, &item.attrs, item.span(), &item.block);
        (self.hot, self.test) = (hot, test);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.function(&item.sig.ident, &item.attrs, item.span(), &item.block);
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        if let Some(body) = &item.default {
            self.function(&item.sig.ident, &item.attrs, item.span(), body);
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

/// The recorded offender a line of `oversized.txt` spells: its subject and measure.
fn entry(line: &str) -> Option<(&str, usize)> {
    let (subject, measure) = line.rsplit_once(' ').filter(|_| !line.starts_with('#'))?;
    Some((subject, measure.parse().ok()?))
}

/// The recorded offenders, as `oversized.txt` spells them.
pub fn parse(listing: &str) -> Measures {
    listing.lines().filter_map(entry).map(|(subject, measure)| (subject.to_owned(), measure)).collect()
}

/// `listing` with each entry that shrank rewritten to its current measure and each that no
/// longer offends dropped, and one report line per change. An entry that grew, a comment and
/// the order of the rest stay as they are.
pub fn lower(listing: &str, current: &Measures) -> (String, Vec<String>) {
    let mut lowered = String::new();
    let mut changes = Vec::new();
    for line in listing.lines() {
        let kept = match entry(line).map(|(subject, recorded)| (subject, recorded, current.get(subject))) {
            Some((subject, recorded, None)) => {
                changes.push(format!("dropped {subject} (was {recorded})"));
                continue;
            }
            Some((subject, recorded, Some(&now))) if now < recorded => {
                changes.push(format!("lowered {subject} {recorded} -> {now}"));
                format!("{subject} {now}")
            }
            _ => line.to_owned(),
        };
        let _ = writeln!(lowered, "{kept}");
    }
    (lowered, changes)
}

/// The rule a measure is born with: `--update` records the first baseline of such a measure that
/// the list has never held, where any other new measure fails until it is fixed.
const BORN_WITH_RULE: &[&str] = &["native_", "concept_", "inline_always_"];

/// Adds the measures of a newly born rule that the baseline has never recorded to `baseline`.
pub fn first_baseline(found: &Measures, recorded: &Measures, baseline: &mut Measures) {
    found
        .iter()
        .filter(|(subject, _)| BORN_WITH_RULE.iter().any(|rule| subject.starts_with(rule)) && !recorded.contains_key(*subject))
        .for_each(|(subject, count)| {
            baseline.insert(subject.clone(), *count);
        });
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
    } else if subject.starts_with("concept_") {
        "one concept, one definition: call the concept's home (`quench-lint/src/concepts.rs`) or read the fact computed once"
    } else if subject.starts_with("inline_always_") {
        "keep only the hit test and load inline: move the fallback into a #[cold] #[inline(never)] function (staged/: inline(never) only outside stencil_build)"
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowering_rewrites_only_shrunk_entries_and_drops_the_vanished() {
        let listing = "# a header, may only shrink\ncomplexity a::f 12\ncomplexity a::g 11\ncomplexity a::h 10\nmagic a::f 3\n";
        let current: Measures = [("complexity a::f", 11), ("complexity a::g", 11), ("magic a::f", 5), ("magic a::new", 1)]
            .into_iter()
            .map(|(subject, measure)| (subject.to_owned(), measure))
            .collect();
        let (lowered, changes) = lower(listing, &current);
        assert_eq!(lowered, "# a header, may only shrink\ncomplexity a::f 11\ncomplexity a::g 11\nmagic a::f 3\n");
        assert_eq!(changes, ["lowered complexity a::f 12 -> 11", "dropped complexity a::h (was 10)"]);
    }

    #[test]
    fn a_born_rule_records_only_measures_the_list_never_held() {
        let found: Measures = [("native_ir a.rs", 2), ("native_ir b.rs", 1), ("magic a::f", 1)]
            .into_iter()
            .map(|(subject, measure)| (subject.to_owned(), measure))
            .collect();
        let recorded: Measures = [("native_ir b.rs".to_owned(), 1)].into_iter().collect();
        let mut baseline = Measures::new();
        first_baseline(&found, &recorded, &mut baseline);
        assert_eq!(baseline.keys().collect::<Vec<_>>(), ["native_ir a.rs"]);
    }
}
