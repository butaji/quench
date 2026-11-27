//! The coverage tests: which opcodes the staged layer decodes, and that tiers derive from it.
#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationExt, OperationOperationRecordsExt, OperationNamesExt};
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

/// Files of `staged/` that name opcodes in code but decode none: no `match` over a record's opcode
/// (`.op()`) and no opcode or class arm (`Op::X =>`, `Class::X |`). A row decodes its records where it names their opcodes;
/// a table-only file that names opcodes without decoding them is a tier's opcode knowledge moved
/// into `staged/`, and counts as debt like any tier file. `wasm_emit/` is the Wasm compiler, which
/// emits opcodes as `compile/` does.
fn undecoding_staged_files(root: &std::path::Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![root.join("staged")];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap().map(Result::unwrap) {
            let path = entry.path();
            if path.is_dir() { pending.push(path); continue; }
            let file = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            if !file.ends_with(".rs") || is_test_file(&file) || file.starts_with("staged/wasm_emit/") { continue; }
            let source = std::fs::read_to_string(&path).unwrap();
            let code = source.split("#[cfg(test)]\nmod ").next().unwrap();
            // Code only: an opcode named in a comment is no knowledge of it.
            let code: String = code.lines().filter(|line| !line.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
            let arm = |prefix: &str| code.match_indices(prefix).any(|(at, _)| {
                let rest = &code[at + prefix.len()..];
                let name_end = rest.find(|c: char| !c.is_ascii_alphanumeric() && c != '_').unwrap_or(rest.len());
                let after = rest[name_end..].trim_start();
                after.starts_with("=>") || after.starts_with('|')
            });
            let names = code.contains("Op::") || code.contains("Class::");
            let decodes = code.contains(".op()") || code.contains("Op::$") || arm("Op::") || arm("Class::");
            if names && !decodes { found.push(file); }
        }
    }
    found
}

/// Opcode lists in the staged layer outside its decode arms: `op == Op::X`, `op != Op::X` and
/// `matches!(op, Op::A | Op::B ..)`. A decode arm (`Op::X => ..`) gives an opcode its meaning once;
/// a list beside it is a second home of the same knowledge, so each opcode such a list names
/// counts (`list:X`), as a tier's would.
fn count_staged_opcode_lists(root: &std::path::Path, words: &Vocabulary, debt: &mut std::collections::BTreeMap<(String, String), usize>) {
    let mut pending = vec![root.join("staged")];
    let mut files = vec!["staged.rs".to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap().map(Result::unwrap) {
            let path = entry.path();
            if path.is_dir() { pending.push(path); continue; }
            files.push(path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
        }
    }
    for file in files {
        if !file.ends_with(".rs") || is_test_file(&file) { continue; }
        let source = std::fs::read_to_string(root.join(&file)).unwrap();
        let code = source.split("\n#[cfg(test)]\nmod ").next().unwrap();
        let code = code.split("#[cfg(test)]\nmod ").next().unwrap();
        let code: String = code.lines().filter(|line| !line.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        let mut add = |name: &str| if words.opcodes.contains(name) { *debt.entry((file.clone(), format!("list:{name}"))).or_insert(0) += 1; };
        let opcode_after = |rest: &str| -> Option<String> {
            let rest = rest.trim_start();
            let rest = rest.strip_prefix("crate::bytecode::").or_else(|| rest.strip_prefix("bytecode::")).unwrap_or(rest);
            rest.strip_prefix("Op::").map(|rest| rest.chars().take_while(|c| ident_char(*c)).collect())
        };
        for operator in ["==", "!="] {
            for (at, _) in code.match_indices(operator) {
                if let Some(name) = opcode_after(&code[at + 2..]) { add(&name); }
            }
        }
        for (at, _) in code.match_indices("matches!(") {
            let body = &code[at + 9..];
            let mut depth = 1usize;
            let end = body.char_indices().find_map(|(i, c)| { match c { '(' => depth += 1, ')' => { depth -= 1; if depth == 0 { return Some(i); } } _ => {} } None }).unwrap_or(body.len());
            let Some((_, patterns)) = body[..end].split_once(',') else { continue };
            let patterns = patterns.split(" if ").next().unwrap();
            let alternatives: Vec<&str> = patterns.split('|').collect();
            // A single-opcode alias the selector test pins (`ALIAS_SELECTORS`) is a checked name of its row.
            if alternatives.len() == 1 && (ALIAS_SELECTORS.contains(&enclosing_function(&code, at)) || CRATE_ALIAS_SELECTORS.contains(&enclosing_function(&code, at))) { continue; }
            for alternative in alternatives {
                if let Some(name) = opcode_after(alternative) { add(&name); }
            }
        }
    }
}

/// The name of the last function `code` opens before `at`: the function a position of a source falls in.
fn enclosing_function(code: &str, at: usize) -> &str {
    code[..at].rmatch_indices("fn ").find(|(start, _)| !code[..*start].ends_with(ident_char)).map_or("", |(start, _)| ident_at(code, start + 3))
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
    // A `#[test]` function is no handler: it checks the lane, whatever its name. A handler that
    // runs its staged step (`step_*_of`), itself or through a function of its own file that calls
    // it, is that step's projection, as a stencil row is (rule (a)).
    let bodies = named_function_bodies(code);
    let lane_staged = lane_handlers_staged(&bodies);
    for (at, _) in code.match_indices("fn lane_") {
        let test = code[..at].trim_end().rsplit('\n').next().is_some_and(|line| line.trim() == "#[test]");
        let name = ident_at(code, at + 3);
        let staged = lane_staged.iter().any(|(function, staged)| *function == name && *staged);
        if !code[..at].ends_with(ident_char) && !test && !staged { add("fn:lane_*".to_owned()); }
    }
    // Hand-written b-jit stencil rows: a function of a `stencils/*_rows.rs` file whose body runs
    // no staged step of its opcode: it neither names `staged::` nor calls a family step
    // (`step_of`, `step_local_of`, ...), one of the staged Wasm operators of `STAGED_WASM_STEPS` or
    // one of the regexp `Machine`'s rows of `STAGED_REGEXP_ROWS`.
    // A call of a staged accessor or helper alone (`object_record_of`, `string_length::..`) is a
    // hand-written row built from staged parts, and still counts. The stencils name rows by code
    // numbers, not opcodes, so the terms above cannot see them.
    // A row may run its step through a function of its own file (a numeric row's `number`, an add
    // row's `add_row`): see `stencil_functions_staged`.
    if file.contains("quench-bjit/stencils/") && file.ends_with("_rows.rs") {
        let staged = stencil_functions_staged(code);
        for runs in staged { if !runs { add("fn:stencil_hand".to_owned()); } }
    }
}

/// Whether each function of `bodies` runs a staged family step (`runs_staged_step`), itself or
/// through one function of the same file that does (a lane handler's shared step wrapper).
fn lane_handlers_staged<'a>(bodies: &[(&'a str, &'a str)]) -> Vec<(&'a str, bool)> {
    let direct: Vec<bool> = bodies.iter().map(|(_, body)| runs_staged_step(body)).collect();
    bodies.iter().map(|(name, body)| (*name, runs_staged_step(body)
        || bodies.iter().zip(&direct).any(|((callee, other), staged)| *staged && other != body && calls_function(body, callee)))).collect()
}

#[test]
fn lane_handlers_run_their_step_directly_or_through_one_own_wrapper() {
    let code = "fn wrap() { step_in_place_call_of(a, b) }\n\
        fn lane_wrapped() { if wrap().is_none() { miss() } }\n\
        fn lane_hand() { r.set(a, b) }\n";
    let bodies = named_function_bodies(code);
    let staged: Vec<bool> = lane_handlers_staged(&bodies).into_iter().map(|(_, staged)| staged).collect();
    assert_eq!(staged, [true, true, false]);
}

/// The staged Wasm operators a stencil row runs as its opcode's step (`staged::wasm`,
/// `wasm_float`, `wasm_memory`, `wasm_call`): the Wasm rows have no `step_*_of` family step.
const STAGED_WASM_STEPS: &[&str] = &["integer_binary", "float_binary", "unary_operator", "load", "store", "call_entry"];

/// The regexp `Machine`'s instruction rows (`quench_regexp::backtrack::machine`, mounted in the
/// stencils as `m`) a regexp stencil row runs as its instruction's step: the regexp rows have no
/// `step_*_of` family step. Named with the module path, so a function of the same name elsewhere
/// is not one.
const STAGED_REGEXP_ROWS: &[&str] = &[
    "m::unit", "m::unit_back", "m::unit_repeat", "m::unit_repeat_back", "m::assertion", "m::split",
    "m::guarded_split", "m::dispatch", "m::capture_open", "m::capture_close", "m::capture_close_back",
    "m::reset_captures", "m::loop_init", "m::loop_head", "m::loop_iterate", "m::empty_check",
    "m::loop_tail", "m::backreference", "m::class_widths", "m::resume",
];

/// Whether `body` calls a staged family step (`step_of`, `step_<family>_of`, with or without a
/// turbofish) or one of `STAGED_WASM_STEPS` or `STAGED_REGEXP_ROWS`, at an identifier boundary.
fn runs_staged_step(body: &str) -> bool {
    let called = |at: usize, name: &str| {
        let rest = &body[at + name.len()..];
        !body[..at].ends_with(ident_char) && !body[..at].ends_with('.') && (rest.starts_with('(') || rest.starts_with("::<"))
    };
    let family = body.match_indices("step_").any(|(at, _)| {
        let name = ident_at(body, at);
        name.ends_with("_of") || name == "step_of"
    } && called(at, ident_at(body, at)));
    family || STAGED_WASM_STEPS.iter().chain(STAGED_REGEXP_ROWS).any(|name| body.match_indices(name).any(|(at, _)| called(at, name)))
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

/// Each function of `code` with a body, by name: the bodies of `function_bodies` with the name that
/// opens each.
fn named_function_bodies(code: &str) -> Vec<(&str, &str)> {
    let mut found = Vec::new();
    for (at, _) in code.match_indices("fn ") {
        if code[..at].ends_with(ident_char) { continue; }
        let Some(open) = code[at..].find(['{', ';']).map(|n| at + n) else { continue };
        if code.as_bytes()[open] == b';' { continue; }
        let name = ident_at(code, at + 3);
        if let Some(body) = function_bodies(&code[at..]).into_iter().next() { found.push((name, body)); }
    }
    found
}

/// Whether each function of a stencil rows file (`named_function_bodies`) runs its staged step: it
/// calls `step_of` / `step_*_of` (or names `staged::`) itself, or reaches one through functions of its
/// own file. A function that calls one such function runs that step; a dispatcher (one that calls
/// more than one function of the file) runs staged steps only where every function it calls does, so
/// one staged route does not clear a router that also reaches a hand-written row.
fn stencil_functions_staged(code: &str) -> Vec<bool> {
    let functions = named_function_bodies(code);
    let direct: Vec<bool> = functions.iter().map(|(_, body)| body.contains("staged::") || runs_staged_step(body)).collect();
    let callees: Vec<Vec<usize>> = functions.iter().map(|(_, body)| (0..functions.len())
        .filter(|&callee| functions[callee].1 != *body && calls_function(body, functions[callee].0)).collect()).collect();
    let mut staged = direct.clone();
    loop {
        let mut grew = false;
        for index in 0..functions.len() {
            if staged[index] || callees[index].is_empty() { continue; }
            if callees[index].iter().all(|&callee| staged[callee]) { staged[index] = true; grew = true; }
        }
        if !grew { break; }
    }
    staged
}

#[test]
fn stencil_rows_reach_their_step_through_own_helpers_only_when_every_route_is_staged() {
    let code = "fn helper() { step_of::<1, _, _>(a, b, c) }\n\
        fn row() { helper() }\n\
        fn hand() { word_at(1) }\n\
        fn mixed() { match k { 0 => row(), _ => hand() } }\n\
        fn routed() { match k { 0 => row(), _ => helper() } }\n";
    assert_eq!(stencil_functions_staged(code), [true, true, false, false, true]);
}

/// Whether `body` calls the function `name` (`name(`, or `name::<..>(`), at an identifier boundary
/// and not as a method.
fn calls_function(body: &str, name: &str) -> bool {
    body.match_indices(name).any(|(at, _)| {
        if body[..at].ends_with(|c: char| ident_char(c) || c == '.') { return false; }
        let rest = body[at + name.len()..].trim_start();
        let rest = match rest.strip_prefix("::<") { Some(generic) => generic.find('>').map_or("", |end| &generic[end + 1..]), None => rest };
        rest.trim_start().starts_with('(')
    })
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
    files.extend(undecoding_staged_files(&root));
    let mut debt = std::collections::BTreeMap::new();
    for file in files {
        if !file.ends_with(".rs") || is_test_file(&file) { continue; }
        let source = std::fs::read_to_string(root.join(&file)).unwrap();
        // Any inline test module closes the production code; its fixtures name opcodes freely.
        let code = source.split("\n#[cfg(test)]\nmod ").next().unwrap();
        let code = code.split("#[cfg(test)]\nmod ").next().unwrap();
        count_decisions(code, &file, &words, &mut debt);
    }
    count_staged_opcode_lists(&root, &words, &mut debt);
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

/// `Op::X` and `Class::X` count per file and name (`op:X`, `Class:X`), so a tier file that names an
/// opcode or a family is debt like a lane handler; a path into `Op` that is no opcode (`Op::COUNT`)
/// and a longer name ending in `Op::` (`WordOp::Add`) are not. The tree holds none today: these
/// tokens are absent from `derivation_debt.txt`, so a first one fails.
#[test]
fn opcode_and_family_names_count_per_file() {
    let words = vocabulary();
    let op = words.opcodes.iter().next().unwrap();
    let class = words.classes.iter().next().unwrap();
    let code = format!("fn f() {{ a == Op::{op} || b == Op::{op} || c == Class::{class} || Op::COUNT == 0 || WordOp::{op} == d }}");
    let mut debt = std::collections::BTreeMap::new();
    count_decisions(&code, "tier.rs", &words, &mut debt);
    assert_eq!(debt.get(&("tier.rs".to_owned(), format!("op:{op}"))), Some(&2));
    assert_eq!(debt.get(&("tier.rs".to_owned(), format!("Class:{class}"))), Some(&1));
    assert_eq!(debt.len(), 2, "{debt:?}");
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

/// The encoders beside their decode arms (`Operation::encode_*`) write records the arm reads back as
/// the same variant with the fields it keeps. Asserted here, not by the encoders' debug assertions,
/// which this profile may compile out.
#[test]
fn staged_encoders_round_trip_through_their_decode_arms() {
    use quench_runtime::staged::Operation;
    type Row = Operation<u64, ()>;
    for (a, b, c, imm) in [(0u16, 1u16, 0u16, 0u32), (3, 7, 2, 5), (9, 4, 1, 40)] {
        let record = Row::encode_move(a, b);
        assert!(matches!(Row::of(record), Some(Operation::Move { destination, source }) if destination == a && source == b), "move {a} {b}");
        let record = Row::encode_jump(imm);
        assert!(matches!(Row::of(record), Some(Operation::Jump)) && record.imm() == imm, "jump {imm}");
        let record = Row::encode_get_field(a, b, c, imm);
        assert!(matches!(Row::of(record), Some(Operation::GetField { destination, .. }) if destination == a), "get_field {a} {b} {c} {imm}: {:?}", Row::of(record).is_some());
        for record in [Row::encode_set_field(a, b, c, imm), Row::encode_set_this_field(a, b, c, imm)] {
            assert!(matches!(Row::of(record), Some(Operation::SetField { value, atom, site, .. })
                if value == a && atom == record.atom_index() && site == record.cache_site_index()), "set_field {a} {b} {c} {imm}");
        }
        let record = Row::encode_load_capture(a, b, c, imm);
        assert!(matches!(Row::of(record), Some(Operation::LoadCapture { destination, depth, slot, .. })
            if destination == a && depth == record.capture_depth() && slot == record.capture_slot()), "load_capture {a} {imm}");
        assert!(matches!(Row::of(Row::encode_call(a, b, c, imm)), Some(Operation::Call(_))), "call {a} {b} {c} {imm}");
        assert!(matches!(Row::of(Row::encode_construct(a, b, c, imm)), Some(Operation::Call(_))), "construct {a} {b} {c} {imm}");
    }
    assert!(matches!(Row::of(Row::encode_nop()), Some(Operation::Nop)), "nop");
    assert!(quench_runtime::staged::operation_is_call_index(Row::encode_call(0, 1, 0, 0).op() as u16));
    assert!(!quench_runtime::staged::operation_is_call_index(Row::encode_nop().op() as u16));
}

/// Each row constant a tier selects a row by (`staged::*_ROW`) names the opcode whose decode arm
/// its doc names: a record of that opcode decodes to that arm's variant. A constant naming another
/// opcode fails here, so a selector is a checked name of its row, not a second home of the opcode.
#[test]
fn row_selectors_decode_to_their_rows() {
    use quench_runtime::bytecode::{Op, WideInstruction};
    use quench_runtime::staged::{self, Operation};
    type Row = Operation<u64, ()>;
    let record = |row: u16, a: u16, b: u16, c: u16, imm: u32| WideInstruction::new(Op::from_index(row as usize).unwrap(), a, b, c, imm);
    let decoded = |row: u16, a: u16, b: u16, c: u16, imm: u32| Row::of(record(row, a, b, c, imm));
    assert!(matches!(decoded(staged::forms::PLAIN_CALL_ROW, 3, 4, 5, 0), Some(Operation::Call(_))), "PLAIN_CALL_ROW");
    // A method call names its site in the program's method-site table, so it decodes against one.
    let sites = [quench_runtime::bytecode::MethodSite::plain(8, 3, 0, 2)];
    let methods = staged::DecodeContext { constants: &[], constant_at: None, method_sites: &sites };
    assert!(matches!(Row::decode(record(staged::METHOD_CALL_ROW, 3, 4, 5, 0), methods), Some(Operation::Call(_))), "METHOD_CALL_ROW");
    assert!(matches!(decoded(staged::COMPARE_JUMP_ROW, 0, 1, 2, 0), Some(Operation::CompareJump { .. })), "COMPARE_JUMP_ROW");
    assert!(matches!(decoded(staged::MOVE_ROW, 1, 2, 0, 0), Some(Operation::Move { destination: 1, source: 2 })), "MOVE_ROW");
    assert!(matches!(decoded(Row::RETURN_ROW, 1, 0, 0, 0), Some(Operation::Return { source: 1 })), "RETURN_ROW");
    assert!((0..8).any(|slot| matches!(decoded(staged::ARGUMENT_GET_ROW, 1, 2, 0, slot), Some(Operation::ArgumentGet { destination: 1, index: 2, .. }))), "ARGUMENT_GET_ROW");
    // An element definition is a literal row: its decoder reads it under the tier's allocation proof.
    assert!(matches!(Row::decode_literal_with(record(Row::DEFINE_ELEMENT_OP as u16, 1, 2, 0, 3), None, ()),
        Some(Operation::DefineElement { array: 2, index: 3, value: 1 })), "DEFINE_ELEMENT_OP");
    // A constant load reads its constant from the program's table, so it decodes against one.
    let constants = [staged::Constant::untyped(quench_runtime::Value::UNDEFINED)];
    let context = staged::DecodeContext { constants: &constants, constant_at: None, method_sites: &[] };
    assert!(matches!(Row::decode(record(staged::LOAD_CONST_ROW, 1, 0, 0, 0), context), Some(Operation::LoadConst { destination: 1, .. })), "LOAD_CONST_ROW");
    for (row, name) in [(staged::BINARY_ROW, "BINARY_ROW"), (staged::NUMERIC_ADD_ROW, "NUMERIC_ADD_ROW"), (staged::NUMERIC_MULTIPLY_ROW, "NUMERIC_MULTIPLY_ROW")] {
        // The immediate names the operator, which a numeric-arithmetic row must hold as its own.
        assert!((0..64).any(|operator| matches!(decoded(row, 1, 2, 3, operator), Some(Operation::Number(_)))), "{name}");
    }
}

/// The staged single-opcode aliases (`staged::jumps`, `staged::calls`, ...) that `alias_selectors_name_one_row`
/// pins: each is true for exactly one opcode, whose record decodes to the operation its doc names. The debt
/// scan counts an alias's opcode as derived only when this list names the alias.
const ALIAS_SELECTORS: &[&str] = &[
    "reads_named_field", "reads_element", "writes_element", "computes_binary", "computes_unary", "loads_constant",
    "converts_numeric", "increments", "calls", "calls_array", "jumps", "returns", "reads_capture", "reads_name",
    "reads_environment_slot", "defines_array_element", "requires_object_coercible", "writes_this_field", "makes_closure",
    "clones_environment", "stores_environment_slot", "writes_environment_slot", "writes_capture", "jumps_when_false",
    "does_nothing", "stores_var_binding", "loads_this", "awaits", "constructs", "selects_string_case",
    "requires_iterator_result", "compare_jump", "binary", "copies_register", "calls_method_site",
];

/// The single-opcode aliases over crate-private operations (the Wasm families), which the crate's
/// `staged::alias_pins` pins as `alias_selectors_name_one_row` pins the public ones.
const CRATE_ALIAS_SELECTORS: &[&str] = &[
    "wasm_branch_table", "wasm_simd", "wasm_simd_shuffle", "wasm_memory_address", "wasm_memory_load", "wasm_memory_store",
    "wasm_f64_unary", "wasm_instance_binding", "wasm_fill_registers",
];

#[test]
fn crate_alias_selectors_name_one_row() {
    let (names, failures) = quench_runtime::staged::alias_pins::crate_alias_failures();
    let pinned: Vec<&str> = names.into_iter().filter(|name| !name.starts_with("wasm_array_new")).collect();
    assert_eq!(pinned, CRATE_ALIAS_SELECTORS, "staged::alias_pins pins CRATE_ALIAS_SELECTORS, in order");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn alias_selectors_name_one_row() {
    use quench_runtime::bytecode::{MethodSite, Op, WideInstruction};
    use quench_runtime::staged::{self, Constant, DecodeContext, Operation};
    type Row = Operation<u64, ()>;
    type Is = fn(&Row) -> bool;
    let aliases: Vec<(&str, fn(Op) -> bool, Is)> = vec![
        ("reads_named_field", staged::reads_named_field, |o| matches!(o, Operation::GetField { .. })),
        ("reads_element", staged::reads_element, |o| matches!(o, Operation::ElementRead { .. } | Operation::ElementReadOperand { .. })),
        ("writes_element", staged::writes_element, |o| matches!(o, Operation::ElementWrite { .. } | Operation::IndexWrite { .. })),
        ("computes_binary", staged::computes_binary, |o| matches!(o, Operation::Binary { .. } | Operation::Number(_) | Operation::Equality { .. })),
        ("computes_unary", staged::computes_unary, |o| matches!(o, Operation::Unary { .. } | Operation::Number(_) | Operation::LogicalNot { .. }
            | Operation::TypeofTest { .. } | Operation::TypeofIndex { .. } | Operation::TypeofString { .. } | Operation::Void { .. })),
        ("loads_constant", staged::loads_constant, |o| matches!(o, Operation::LoadConst { .. })),
        ("converts_numeric", staged::converts_numeric, |o| matches!(o, Operation::Number(_))),
        ("increments", staged::increments, |o| matches!(o, Operation::Number(_) | Operation::IncDecLocal { .. })),
        ("calls", staged::calls, |o| matches!(o, Operation::Call(_))),
        ("calls_array", staged::calls_array, |o| matches!(o, Operation::SpreadCall { .. })),
        ("jumps", staged::jumps, |o| matches!(o, Operation::Jump)),
        ("returns", staged::returns, |o| matches!(o, Operation::Return { .. })),
        ("reads_capture", staged::reads_capture, |o| matches!(o, Operation::LoadCapture { .. })),
        ("reads_name", staged::reads_name, |o| matches!(o, Operation::LoadName { .. })),
        ("reads_environment_slot", staged::reads_environment_slot, |o| matches!(o, Operation::LoadEnvLocal { .. })),
        ("defines_array_element", staged::defines_array_element, |o| matches!(o, Operation::DefineElement { .. })),
        ("requires_object_coercible", staged::requires_object_coercible, |o| matches!(o, Operation::RequireObjectCoercible { .. })),
        ("writes_this_field", staged::writes_this_field, |o| matches!(o, Operation::SetField { .. })),
        ("makes_closure", staged::makes_closure, |o| matches!(o, Operation::Allocate { .. })),
        ("clones_environment", staged::clones_environment, |o| matches!(o, Operation::CloneEnv { .. })),
        ("stores_environment_slot", staged::stores_environment_slot, |o| matches!(o, Operation::StoreEnvLocal { .. })),
        ("writes_environment_slot", staged::writes_environment_slot, |o| matches!(o, Operation::StoreEnvLocal { .. })),
        ("writes_capture", staged::writes_capture, |o| matches!(o, Operation::StoreCapture { .. })),
        ("jumps_when_false", staged::jumps_when_false, |o| matches!(o, Operation::JumpIf { taken: false, .. })),
        ("does_nothing", staged::does_nothing, |o| matches!(o, Operation::Nop)),
        ("stores_var_binding", staged::stores_var_binding, |o| matches!(o, Operation::Protocol { .. })),
        ("loads_this", staged::loads_this, |o| matches!(o, Operation::LoadThis { .. })),
        ("awaits", staged::awaits, |o| matches!(o, Operation::Suspend { .. })),
        ("constructs", staged::constructs, |o| matches!(o, Operation::Call(_) | Operation::SuperCall { .. })),
        ("selects_string_case", staged::selects_string_case, |o| matches!(o, Operation::StringCaseIndex { .. })),
        ("requires_iterator_result", staged::requires_iterator_result, |o| matches!(o, Operation::RequireIteratorResult { .. })),
        ("compare_jump", staged::compare_jump, |o| matches!(o, Operation::CompareJump { .. })),
        ("binary", staged::binary, |o| matches!(o, Operation::Binary { .. } | Operation::Number(_) | Operation::Equality { .. })),
        ("copies_register", staged::copies_register, |o| matches!(o, Operation::Move { .. })),
        ("calls_method_site", staged::calls_method_site, |o| matches!(o, Operation::Call(_))),
    ];
    assert_eq!(aliases.iter().map(|(name, ..)| *name).collect::<Vec<_>>(), ALIAS_SELECTORS, "the test's aliases are ALIAS_SELECTORS, in order");
    let constants = [Constant::untyped(quench_runtime::Value::UNDEFINED); 4];
    let sites = [MethodSite::plain(8, 3, 0, 2); 4];
    let contexts = [DecodeContext::EMPTY, DecodeContext { constants: &constants, constant_at: None, method_sites: &sites }];
    let decodes = |op: Op, is: Is| {
        (0..48u32).any(|imm| [(1u16, 2u16, 3u16), (0, 1, 0), (1, 0, 0)].iter().any(|&(a, b, c)| {
            let record = WideInstruction::new(op, a, b, c, imm);
            contexts.iter().any(|&context| Row::decode(record, context).is_some_and(|o| is(&o)))
                || Row::decode_literal_with(record, None, ()).is_some_and(|o| is(&o))
                || Row::decode_lane(record).is_some_and(|o| is(&o))
                || Row::decode_load_name(record, ()).is_some_and(|o| is(&o))
        }))
    };
    let mut failures = Vec::new();
    for (name, alias, is) in &aliases {
        let ops: Vec<Op> = (0..Op::COUNT).map(|index| Op::from_index(index).unwrap()).filter(|&op| alias(op)).collect();
        match ops.as_slice() {
            [op] if decodes(*op, *is) => {}
            [op] => failures.push(format!("{name}: {} does not decode to the documented operation", Op::NAMES[*op as usize])),
            _ => failures.push(format!("{name}: true for {} opcodes, not one", ops.len())),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
