//! Fails when the repository says the same thing twice.
//!
//! - `duplicate-function`: two functions whose bodies are token-identical (at least
//!   [`MIN_BODY_TOKENS`] tokens) do the same work; keep one and call it.
//! - `duplicate-constant`: two module-level `const` items with the same name, type and value
//!   store the same fact; keep one authority and import it. A constant local to a function
//!   names a value for that function alone, and an integration test's constants are its
//!   fixture, so neither is compared.
//!
//! usage: quench-lint [ROOT...]   (default: crates quench-bench/src tools)

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use quote::ToTokens;
use syn::visit::Visit;

/// Smaller bodies are accessors and forwarding shims whose repetition carries no knowledge.
const MIN_BODY_TOKENS: usize = 40;
const DEFAULT_ROOTS: &[&str] = &["crates", "quench-bench/src", "tools"];
/// Checked-out third-party corpora and build output, not Quench code.
const SKIPPED_DIRECTORIES: &[&str] = &[
    "target",
    "testsuite",
    "node_modules",
    "jetstream",
    "js-engine-benchmark",
];

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rule {
    DuplicateFunction,
    DuplicateConstant,
}

impl Rule {
    fn name(self) -> &'static str {
        match self {
            Rule::DuplicateFunction => "duplicate-function",
            Rule::DuplicateConstant => "duplicate-constant",
        }
    }
}

struct Site {
    file: String,
    line: usize,
    name: String,
}

type Findings = BTreeMap<(Rule, String), Vec<Site>>;

struct Collector<'a> {
    file: &'a str,
    /// Items this file defines: a body naming one of them may resolve differently from a
    /// token-identical body in another file, so the name is qualified by its file.
    local_items: &'a HashSet<String>,
    /// How many function bodies enclose the item being visited.
    function_depth: usize,
    findings: &'a mut Findings,
}

/// Collects the names of the items a file defines, at any module depth.
#[derive(Default)]
struct LocalItems(HashSet<String>);

impl<'ast> Visit<'ast> for LocalItems {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let ident = match item {
            syn::Item::Const(item) => Some(&item.ident),
            syn::Item::Enum(item) => Some(&item.ident),
            syn::Item::Fn(item) => Some(&item.sig.ident),
            syn::Item::Macro(item) => item.ident.as_ref(),
            syn::Item::Static(item) => Some(&item.ident),
            syn::Item::Struct(item) => Some(&item.ident),
            syn::Item::Trait(item) => Some(&item.ident),
            syn::Item::Type(item) => Some(&item.ident),
            syn::Item::Union(item) => Some(&item.ident),
            _ => None,
        };
        self.0.extend(ident.map(ToString::to_string));
        syn::visit::visit_item(self, item);
    }
}

impl Collector<'_> {
    fn record(&mut self, rule: Rule, key: String, ident: &syn::Ident) {
        let site = Site {
            file: self.file.to_owned(),
            line: ident.span().start().line,
            name: ident.to_string(),
        };
        self.findings.entry((rule, key)).or_default().push(site);
    }
}

impl<'ast> Visit<'ast> for Collector<'_> {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.function(&item.sig, &item.block);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.function(&item.sig, &item.block);
    }

    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        if item.ident != "_" && self.function_depth == 0 && !self.file.contains("/tests/") {
            let mut key = format!(
                "{}: {} = {}",
                item.ident,
                item.ty.to_token_stream(),
                item.expr.to_token_stream()
            );
            // A macro (`include_str!`, `env!`) expands against its own file and crate.
            if matches!(*item.expr, syn::Expr::Macro(_)) {
                key.push_str(&format!(" @{}", self.file));
            }
            self.record(Rule::DuplicateConstant, key, &item.ident);
        }
        syn::visit::visit_item_const(self, item);
    }
}

impl Collector<'_> {
    fn function(&mut self, signature: &syn::Signature, body: &syn::Block) {
        let mut text = String::new();
        if self.flatten(body.to_token_stream(), &mut text) >= MIN_BODY_TOKENS {
            self.record(Rule::DuplicateFunction, text, &signature.ident);
        }
        self.function_depth += 1;
        syn::visit::visit_block(self, body);
        self.function_depth -= 1;
    }

    /// Appends a canonical spelling of `tokens` to `text` and returns how many tokens it held.
    fn flatten(&self, tokens: TokenStream, text: &mut String) -> usize {
        tokens
            .into_iter()
            .map(|token| match token {
                TokenTree::Group(group) => {
                    let (open, close) = match group.delimiter() {
                        Delimiter::Brace => ("{", "}"),
                        Delimiter::Parenthesis => ("(", ")"),
                        Delimiter::Bracket => ("[", "]"),
                        Delimiter::None => ("", ""),
                    };
                    text.push_str(open);
                    let inner = self.flatten(group.stream(), text);
                    text.push_str(close);
                    inner + 1
                }
                TokenTree::Ident(ident) if self.local_items.contains(&ident.to_string()) => {
                    text.push_str(&format!("{ident}@{} ", self.file));
                    1
                }
                other => {
                    text.push_str(&other.to_string());
                    text.push(' ');
                    1
                }
            })
            .sum()
    }
}

fn rust_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        let skipped = path
            .file_name()
            .is_some_and(|name| SKIPPED_DIRECTORIES.iter().any(|skip| name == *skip));
        if path.is_dir() && !skipped {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let roots: Vec<&str> = if arguments.is_empty() {
        DEFAULT_ROOTS.to_vec()
    } else {
        arguments.iter().map(String::as_str).collect()
    };
    let mut files = Vec::new();
    roots
        .iter()
        .for_each(|root| rust_files(Path::new(root), &mut files));
    files.sort();

    let mut findings = Findings::new();
    for path in &files {
        let file = path.display().to_string();
        let parsed = fs::read_to_string(path)
            .map_err(|error| error.to_string())
            .and_then(|source| {
                syn::parse_file(&source)
                    .map_err(|error| format!("{}:{}", error.span().start().line, error))
            });
        match parsed {
            Ok(syntax) => {
                let mut local_items = LocalItems::default();
                local_items.visit_file(&syntax);
                Collector {
                    file: &file,
                    local_items: &local_items.0,
                    function_depth: 0,
                    findings: &mut findings,
                }
                .visit_file(&syntax);
            }
            Err(error) => {
                eprintln!("quench-lint: cannot read {file}: {error}");
                return ExitCode::FAILURE;
            }
        }
    }

    let mut violations = 0;
    for ((rule, _), sites) in findings.iter().filter(|(_, sites)| sites.len() > 1) {
        violations += 1;
        println!("{}: {} copies", rule.name(), sites.len());
        for site in sites {
            println!("  {}:{} {}", site.file, site.line, site.name);
        }
    }
    if violations == 0 {
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "quench-lint: {violations} redundancies in {} files",
            files.len()
        );
        ExitCode::FAILURE
    }
}
