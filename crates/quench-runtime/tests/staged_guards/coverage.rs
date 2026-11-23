//! The coverage tests: which opcodes the staged layer decodes, and that tiers derive from it.
/// Hand-written decodes of the object and narrow layouts outside `staged`, per native-tier
/// file: each is a second home of a rule `staged` owns (AGENTS.md, manifesto 4). The counts
/// only go down; a new decode belongs in `staged` as an operation every tier runs.
const LAYOUT_DECODES: &[(&str, &str, usize)] = &[
    ("compiled.rs", include_str!("../../../quench-runtime-engine/src/compiled.rs"), 0),
    ("compiled/lane_region.rs", include_str!("../../../quench-runtime-engine/src/compiled/lane_region.rs"), 0),
    ("compiled/osr_loop.rs", include_str!("../../../quench-runtime-engine/src/compiled/osr_loop.rs"), 0),
    ("compiled/object_slots.rs", include_str!("../../../quench-runtime-engine/src/compiled/object_slots.rs"), 0),
    ("js_compiled.rs", include_str!("../../../quench-runtime-engine/src/js_compiled.rs"), 0),
    ("dispatch_fast.rs", include_str!("../../../quench-runtime-engine/src/dispatch_fast.rs"), 0),
];

/// The layout facts whose use is a decode: the bits a reader tests to learn a storage form or
/// a narrow value's meaning.
const LAYOUT_TOKENS: &[&str] = &[
    "extras_tag_mask",
    "inline_storage_mask",
    "storage_start_mask",
    "NarrowLayout::INTEGER_FLAG",
    "NarrowLayout::CODE_SHIFT",
    "proto_mask",
    "proto_record_base",
];

#[test]
fn staged_coverage_layout_decodes_outside_staged_only_shrink() {
    for (file, source, allowed) in LAYOUT_DECODES {
        let found: usize = LAYOUT_TOKENS.iter().map(|token| source.matches(token).count()).sum();
        assert!(found <= *allowed,
            "{file} decodes object or narrow layouts {found} times (allowed {allowed}): route the read through `staged`");
        assert!(found >= *allowed,
            "{file} now decodes layouts {found} times, below {allowed}: lower its allowance in LAYOUT_DECODES");
    }
}

// ---------------------------------------------------------------------------------------------
// One semantics, every tier derived (Futamura): the derivation debt.
//
// An opcode's meaning lives in exactly one place, its staged row (`staged::step` over `Stage`).
// The interpreter's handlers are that row on `Concrete`, the b-jit stencils the row on
// `StencilStage`, u-jit and AOT the row on `Emit`. Any other code that singles out an opcode or
// an opcode family decides its meaning by hand, a second definition that can drift.
//
// The debt counts every spelling such a decision can take, so renaming does not pay it off:
// `Op::X`, the opcode-index constants (`index::X` and any `use bytecode::index as ...` alias),
// the catalog's opcode predicates (`.is_get_field()` ...), and the family tags `Class::X` and
// `JsDenotation::X`; plus every hand-written lane handler (`fn lane_*` outside a macro), and every
// opcode with no staged row. Only `staged/`, the catalog (`bytecode/`) and the compiler that
// emits opcodes (`compile/`, apart from the peephole rewriter) may name an opcode. `staged/derivation_debt.txt` lists the debt per
// file and token; it may only shrink. `QUENCH_DEBT_WRITE=1` rewrites it after a shrink.
// ---------------------------------------------------------------------------------------------

/// The trees whose code may execute, lower, profile or link opcodes, relative to `src`: this crate's
/// whole source, the engine crate that holds the lane and compiled tiers, and the b-jit crate. The
/// scan covers each tree whole, so a tier in any file of them is debt; a directory list would miss
/// a tier that a crate split moved out of it.
const DEBT_ROOTS: &[&str] = &["", "../../quench-runtime-engine/src", "../../quench-bjit/src", "../../quench-bjit/stencils"];

/// Directories of this crate's source the scan leaves out: `staged/` holds the rows, `bytecode/` is
/// the catalog that defines the vocabulary, and `compile/` emits opcodes.
const DEBT_EXEMPT_DIRECTORIES: &[&str] = &["staged", "bytecode", "compile"];

/// Files of this crate's source the scan leaves out: the module roots of `staged` and `bytecode`.
const DEBT_EXEMPT_FILES: &[&str] = &["staged.rs", "bytecode.rs"];

/// Files inside the exempt directories that single out opcodes: the peephole rewriters and the
/// validator. They are scanned on their own.
const DEBT_FILES: &[&str] = &["compile/rewrite.rs", "compile/deferred_rewrite.rs", "bytecode/validate.rs"];

/// Whether `file` (relative to `src`, or `../` for another crate) is left out of the walk.
fn debt_exempt(file: &str) -> bool {
    !file.starts_with("../") && (DEBT_EXEMPT_FILES.contains(&file)
        || DEBT_EXEMPT_DIRECTORIES.iter().any(|directory| file.starts_with(&format!("{directory}/"))))
}

/// Whether `file` (relative to `src`) holds only tests: a `tests` or `*_tests` module file or directory.
fn is_test_file(file: &str) -> bool {
    file.split('/').map(|part| part.strip_suffix(".rs").unwrap_or(part)).any(|part| part == "tests" || part.ends_with("_tests"))
}

fn src_root() -> std::path::PathBuf { std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src") }

/// The `is_*(self)` methods of every `impl Op` block in `source`: opcode predicates.
fn op_predicate_methods(source: &str, into: &mut std::collections::BTreeSet<String>) {
    for block in source.split("\nimpl Op {").skip(1) {
        let body = block.split("\n}\n").next().unwrap_or(block);
        for (at, _) in body.match_indices("fn is_") {
            let name: String = body[at + 3..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            if body[at + 3 + name.len()..].starts_with("(self)") { into.insert(name); }
        }
    }
    // The catalog's generated predicates: `is_x => X,` lines of `op_predicates!`.
    if let Some(list) = source.split("op_predicates! {").nth(1) {
        for line in list.split("\n}").next().unwrap().lines() {
            let line = line.trim();
            if let Some((name, _)) = line.split_once(" =>") {
                if name.starts_with("is_") { into.insert(name.to_owned()); }
            }
        }
    }
}

/// The variants of `enum name` declared in `source`.
fn enum_variants(source: &str, name: &str) -> Vec<String> {
    let body = source.split(&format!("enum {name} {{")).nth(1).unwrap().split("\n}").next().unwrap();
    body.lines().map(str::trim).filter(|line| line.starts_with(|c: char| c.is_ascii_uppercase()))
        .map(|line| line.chars().take_while(char::is_ascii_alphanumeric).collect()).collect()
}

/// The vocabulary that names an opcode or an opcode family.
struct Vocabulary {
    opcodes: std::collections::BTreeSet<&'static str>,
    predicates: std::collections::BTreeSet<String>,
    classes: std::collections::BTreeSet<String>,
    denotations: std::collections::BTreeSet<String>,
}

fn vocabulary() -> Vocabulary {
    let root = src_root();
    let read = |file: &str| std::fs::read_to_string(root.join(file)).unwrap();
    let mut predicates = std::collections::BTreeSet::new();
    for file in ["bytecode.rs", "bytecode/op_classes.rs", "bytecode/field_store.rs", "bytecode/op_catalog.rs"] {
        op_predicate_methods(&read(file), &mut predicates);
    }
    Vocabulary {
        opcodes: quench_runtime::bytecode::Op::NAMES.iter().copied().collect(),
        predicates,
        classes: enum_variants(&read("bytecode/op_catalog.rs"), "Class").into_iter().filter(|v| v != "Other").collect(),
        denotations: enum_variants(&read("bytecode/denotation.rs"), "JsDenotation").into_iter().collect(),
    }
}

fn ident_char(c: char) -> bool { c.is_ascii_alphanumeric() || c == '_' }

/// The identifier that follows `at` in `code`.
fn ident_at(code: &str, at: usize) -> &str {
    let end = code[at..].find(|c: char| !ident_char(c)).map_or(code.len(), |n| at + n);
    &code[at..end]
}

/// Adds each opcode decision in `code` to `debt` under `file`.
fn count_decisions(code: &str, file: &str, words: &Vocabulary, debt: &mut std::collections::BTreeMap<(String, String), usize>) {
    let mut add = |token: String| *debt.entry((file.to_owned(), token)).or_insert(0) += 1;
    // Opcode identities: `Op::X`, `index::X` and the file's own aliases of the index module.
    let mut paths = vec!["Op::".to_owned(), "index::".to_owned()];
    for (at, _) in code.match_indices("index as ") {
        paths.push(format!("{}::", ident_at(code, at + 9)));
    }
    for path in &paths {
        for (at, _) in code.match_indices(path.as_str()) {
            if code[..at].ends_with(ident_char) { continue; }
            let name = ident_at(code, at + path.len());
            if words.opcodes.contains(name) { add(format!("op:{name}")); }
        }
    }
    // Opcode predicates (`.is_get_field()`), and family tags.
    for (at, _) in code.match_indices(".is_") {
        let name = ident_at(code, at + 1);
        if words.predicates.contains(name) && code[at + 1 + name.len()..].starts_with('(') { add(format!("is:{name}")); }
    }
    for (tag, variants) in [("Class::", &words.classes), ("JsDenotation::", &words.denotations)] {
        for (at, _) in code.match_indices(tag) {
            if code[..at].ends_with(ident_char) { continue; }
            let name = ident_at(code, at + tag.len());
            if variants.contains(name) { add(format!("{}{name}", &tag[..tag.len() - 1])); }
        }
    }
    // Hand-written lane handlers: a `fn lane_` a macro did not generate.
    for (at, _) in code.match_indices("fn lane_") {
        if !code[..at].ends_with(ident_char) { add("fn:lane_*".to_owned()); }
    }
    // Hand-written b-jit stencil rows: a function of a `stencils/*_rows.rs` file whose body runs
    // no staged row: it neither names `staged::` nor calls a function or module the file imports
    // from `crate::staged` (`step_of(..)`, `object::instanceof_by_words(..)`). The stencils name rows
    // by code numbers, not opcodes, so the terms above cannot see them.
    if file.contains("quench-bjit/stencils/") && file.ends_with("_rows.rs") {
        let staged = staged_imports(code);
        for body in function_bodies(code) {
            if !body.contains("staged::") && !calls_any(body, &staged) { add("fn:stencil_hand".to_owned()); }
        }
    }
}

/// The functions and modules `code` imports from `crate::staged`: the lower-case names of its
/// `use crate::staged::...` items (types, traits and constants are not runs of a row).
fn staged_imports(code: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in code.lines().map(str::trim).filter(|line| line.starts_with("use crate::staged")) {
        let items = line.trim_start_matches("use crate::staged").trim_end_matches(';');
        let items = items.split('{').next_back().unwrap_or(items).trim_end_matches('}');
        for item in items.split(',') {
            // `a::b`, `a::{self, b}` and `a as b` name the last segment or the alias.
            let item = item.trim().rsplit(" as ").next().unwrap_or("").trim();
            let name = item.rsplit("::").next().unwrap_or(item).trim();
            let name = if name == "self" { item.trim_end_matches("::self").rsplit("::").next().unwrap_or("") } else { name };
            let lower = name.chars().next().is_some_and(|c| c.is_ascii_lowercase());
            if lower && name != "op_kind" && name.chars().all(ident_char) { names.push(name.to_owned()); }
        }
    }
    names
}

/// Whether `body` calls one of `names` (`name(` or `name::`, at an identifier boundary).
fn calls_any(body: &str, names: &[String]) -> bool {
    names.iter().any(|name| body.match_indices(name.as_str()).any(|(at, _)| {
        let rest = &body[at + name.len()..];
        !body[..at].ends_with(ident_char) && !body[..at].ends_with('.') && (rest.starts_with('(') || rest.starts_with("::") || rest.starts_with("::<"))
    }))
}

/// The bodies (from the opening brace to its match) of the functions `code` defines.
fn function_bodies(code: &str) -> Vec<&str> {
    let mut bodies = Vec::new();
    for (at, _) in code.match_indices("fn ") {
        if code[..at].ends_with(ident_char) { continue; }
        // The signature ends at the first `{` or `;` (a trait item with no body).
        let Some(open) = code[at..].find(['{', ';']).map(|n| at + n) else { continue };
        if code.as_bytes()[open] == b';' { continue; }
        let mut depth = 0usize;
        for (offset, c) in code[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 { bodies.push(&code[open..open + offset + 1]); break; }
                }
                _ => {}
            }
        }
    }
    bodies
}

/// The derivation debt of the tree, per (file, token).
fn derivation_debt() -> std::collections::BTreeMap<(String, String), usize> {
    let root = src_root();
    let words = vocabulary();
    let mut pending: Vec<std::path::PathBuf> = DEBT_ROOTS.iter().map(|directory| root.join(directory)).collect();
    // Keyed by path relative to `src`, so a file both walked and listed in `DEBT_FILES` counts once.
    let mut files: std::collections::BTreeSet<String> = DEBT_FILES.iter().map(|file| file.to_string()).collect();
    while let Some(directory) = pending.pop() {
        // A missing root is a moved tree: the scan must fail there, not read nothing.
        let entries = std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("derivation debt root {}: {error}", directory.display()));
        for entry in entries.map(Result::unwrap) {
            let path = entry.path();
            if path.is_dir() { pending.push(path); continue; }
            let file = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            if !debt_exempt(&file) { files.insert(file); }
        }
    }
    let mut debt = std::collections::BTreeMap::new();
    for file in files {
        if !file.ends_with(".rs") || is_test_file(&file) { continue; }
        let source = std::fs::read_to_string(root.join(&file)).unwrap();
        // Any inline test module closes the production code; its fixtures name opcodes freely.
        let code = source.split("\n#[cfg(test)]\nmod ").next().unwrap();
        let code = code.split("#[cfg(test)]\nmod ").next().unwrap();
        count_decisions(code, &file, &words, &mut debt);
    }
    // Opcodes with no staged row: a JS opcode the catalog gives no staged decoder, a Wasm opcode
    // no `wasm_step::Step` describes. An op the catalog marks `NO_STEP` has no meaning to derive:
    // the wide prefix (`Wide`) is never dispatched, and `Nop` does nothing.
    for index in 0..quench_runtime::bytecode::Op::COUNT {
        let op = quench_runtime::bytecode::Op::from_index(index).unwrap();
        let staged = quench_runtime::staged::op_has_row(op);
        if !staged { debt.insert(("<catalog>".to_owned(), format!("unstaged:{}", quench_runtime::bytecode::Op::NAMES[index])), 1); }
    }
    debt
}

const DEBT_FILE: &str = include_str!("../../src/staged/derivation_debt.txt");

fn render_debt(debt: &std::collections::BTreeMap<(String, String), usize>) -> String {
    let total: usize = debt.values().sum();
    let mut out = format!("# Derivation debt: each place a tier decides an opcode's meaning outside its staged row\n\
        # (`tests/staged_guards/coverage.rs`, `tiers_derive_opcode_meaning_from_the_interpreter`). Lines are\n\
        # `file token count`; counts may only shrink, and a new line fails. Goal: an empty file.\n\
        # total {total}\n");
    for ((file, token), count) in debt { out.push_str(&format!("{file} {token} {count}\n")); }
    out
}

/// Futamura's projections, enforced: every tier's meaning of an opcode is its staged row,
/// specialized. The debt of hand-written decisions only shrinks, and a new one fails here.
#[test]
fn tiers_derive_opcode_meaning_from_the_interpreter() {
    // `QUENCH_DEBT_WRITE=init` records the debt as it stands (the first baseline only).
    if std::env::var("QUENCH_DEBT_WRITE").as_deref() == Ok("init") {
        std::fs::write(src_root().join("staged/derivation_debt.txt"), render_debt(&derivation_debt())).unwrap();
        return;
    }
    let allowed: std::collections::BTreeMap<(String, String), usize> = DEBT_FILE.lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let mut fields = line.split(' ');
            let (file, token, count) = (fields.next().unwrap(), fields.next().unwrap(), fields.next().unwrap());
            ((file.to_owned(), token.to_owned()), count.parse().unwrap())
        })
        .collect();
    let found = derivation_debt();
    let grew: Vec<String> = found.iter().filter(|(key, count)| **count > allowed.get(*key).copied().unwrap_or(0))
        .map(|((file, token), count)| format!("{file} {token}: {count} (allowed {})", allowed.get(&(file.clone(), token.clone())).copied().unwrap_or(0)))
        .collect();
    assert!(grew.is_empty(), "new hand-written opcode meaning; derive it from the opcode's staged row instead:\n{}", grew.join("\n"));
    if found != allowed {
        if std::env::var_os("QUENCH_DEBT_WRITE").is_some() {
            std::fs::write(src_root().join("staged/derivation_debt.txt"), render_debt(&found)).unwrap();
        } else {
            panic!("derivation debt shrank ({} -> {}): rerun with QUENCH_DEBT_WRITE=1 to lower staged/derivation_debt.txt",
                allowed.values().sum::<usize>(), found.values().sum::<usize>());
        }
    }
}
