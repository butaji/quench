//! Pattern dispatch over duplicated decision trees: a case analysis is written once.
//!
//! - `decision-twin`: two `match` expressions with at least [`MIN_ARMS`] arms whose arms
//!   (patterns, guards and results) are token-identical decide the same thing twice; keep one
//!   and call it.
//! - `decision-column`: two `match` expressions over the same patterns whose every arm only
//!   names a value (a literal or a path) are two columns of one table split across functions;
//!   make the table one row per case and derive both views from it.
//!
//! Each group is recorded by its first site with the number of copies; it joins the
//! shrink-only measures in `oversized.txt`.

use std::collections::{BTreeMap, BTreeSet};

use quote::ToTokens;
use syn::visit::Visit;

/// Smaller matches are ordinary two-way branches whose repetition carries no knowledge.
const MIN_ARMS: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rule {
    Twin,
    Column,
}

impl Rule {
    fn name(self) -> &'static str {
        match self {
            Rule::Twin => "decision-twin",
            Rule::Column => "decision-column",
        }
    }

    fn remedy(self) -> &'static str {
        match self {
            Rule::Twin => "decide once in a named function and call it",
            Rule::Column => "make one table with a row per case and derive each view from it",
        }
    }
}

/// Every non-trivial case analysis seen so far, by what it decides, with the functions it
/// appears in.
#[derive(Default)]
pub struct DecisionTrees(BTreeMap<(Rule, String), Vec<Site>>);

/// Where a case analysis is written, and its arms as tokens.
struct Site {
    place: String,
    arms: String,
}

impl DecisionTrees {
    /// Records the matches in one function's body.
    pub fn scan(&mut self, subject: &str, body: &syn::Block) {
        if crate::in_test_path(subject) {
            return;
        }
        let mut matches = Matches { subject, trees: self };
        matches.visit_block(body);
    }

    /// Prints every decision made more than once; returns how many.
    pub fn report(&self) -> usize {
        let repeated: Vec<_> = self.0.iter().filter(|((rule, _), sites)| copies(*rule, sites) > 1).collect();
        for ((rule, _), sites) in &repeated {
            println!("{}: {} copies; {}", rule.name(), copies(*rule, sites), rule.remedy());
            sites.iter().for_each(|site| println!("  {}", site.place));
        }
        repeated.len()
    }
}

/// How often a decision is made: every site for a twin; for a column, the distinct tables
/// (identical tables are one twin, reported as such).
fn copies(rule: Rule, sites: &[Site]) -> usize {
    match rule {
        Rule::Twin => sites.len(),
        Rule::Column => sites.iter().map(|site| &site.arms).collect::<BTreeSet<_>>().len(),
    }
}

struct Matches<'a> {
    subject: &'a str,
    trees: &'a mut DecisionTrees,
}

fn tokens(node: &impl ToTokens) -> String {
    node.to_token_stream().to_string()
}

/// Whether an arm's result only names a value, making the match a lookup table.
fn names_a_value(body: &syn::Expr) -> bool {
    match body {
        syn::Expr::Lit(_) | syn::Expr::Path(_) => true,
        syn::Expr::Unary(unary) => names_a_value(&unary.expr),
        syn::Expr::Paren(paren) => names_a_value(&paren.expr),
        syn::Expr::Block(block) => match block.block.stmts.as_slice() {
            [syn::Stmt::Expr(expr, None)] => names_a_value(expr),
            _ => false,
        },
        _ => false,
    }
}

/// Whether a pattern names a case of a domain type (an enum variant, a struct), as opposed to
/// literals and wildcards, which every unrelated table over the same numbers shares.
fn names_a_case(pattern: &syn::Pat) -> bool {
    match pattern {
        syn::Pat::Path(_) | syn::Pat::TupleStruct(_) | syn::Pat::Struct(_) => true,
        syn::Pat::Or(or) => or.cases.iter().any(names_a_case),
        syn::Pat::Tuple(tuple) => tuple.elems.iter().any(names_a_case),
        syn::Pat::Reference(reference) => names_a_case(&reference.pat),
        _ => false,
    }
}

impl<'ast> Visit<'ast> for Matches<'_> {
    fn visit_expr_match(&mut self, decision: &'ast syn::ExprMatch) {
        if decision.arms.len() >= MIN_ARMS {
            let line = decision.match_token.span.start().line;
            let arms = decision.arms.iter().map(tokens).collect::<Vec<_>>().join(" ; ");
            self.record(Rule::Twin, arms.clone(), &arms, line);
            let table = decision.arms.iter().all(|arm| arm.guard.is_none() && names_a_value(&arm.body));
            if table && decision.arms.iter().any(|arm| names_a_case(&arm.pat)) {
                let mut patterns: Vec<String> = decision.arms.iter().map(|arm| tokens(&arm.pat)).collect();
                patterns.sort();
                self.record(Rule::Column, patterns.join(" | "), &arms, line);
            }
        }
        syn::visit::visit_expr_match(self, decision);
    }

    /// A nested function is its own subject.
    fn visit_item(&mut self, _: &'ast syn::Item) {}
}

impl Matches<'_> {
    fn record(&mut self, rule: Rule, key: String, arms: &str, line: usize) {
        let site = Site { place: format!("{} (line {line})", self.subject), arms: arms.to_owned() };
        self.trees.0.entry((rule, key)).or_default().push(site);
    }
}
