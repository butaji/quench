//! The fixpoint-driver ratchet: one forward-meet driver, `dataflow::solve`, runs every analysis to its
//! fixpoint (SSOT 5, TOD-2235). A hand-written worklist loop beside it is a second driver, with its own
//! visiting order and termination argument, which can drift from the shared one.
//!
//! A driver is found by its shape, not its name. A worklist loop is a
//! `while let Some(x) = W.pop()` head (or `pop_front`, `pop_back`, `pop_first`, `pop_last`, or
//! `W.iter().next()`, the bit-set form `solve` uses). It counts as a fixpoint driver when the function that
//! holds it, after that head, both puts work back on the same `W` (`W.push(`, `W.push_back(`,
//! `W.push_front(` or `W.insert(`) and joins lattice states: it calls a function or method named
//! `meet`, `join`, `merge` or `widen`, or one whose name starts with one of these and an underscore
//! (`join_from`, `meet_rows`). A traversal that only marks nodes visited (reachability, a DFS) joins
//! nothing and does not count; neither does a join with no worklist. Each loop head counts once.
//!
//! `staged/fixpoint_debt.txt` holds the count per file; counts may only shrink, and a new file fails.
//! `QUENCH_FIXPOINT_WRITE=1` rewrites it after a driver moves onto `dataflow::solve`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The trees scanned, relative to `crates/`: this crate's whole source (the AOT compiler included), the
/// engine crate and the b-jit.
const DRIVER_DIRECTORIES: &[&str] = &["quench-runtime/src", "quench-runtime-engine/src", "quench-bjit/src", "quench-bjit/stencils"];

/// The one driver: the file that holds `dataflow::solve`.
const HOME: &str = "quench-runtime/src/dataflow.rs";

const DEBT_FILE: &str = include_str!("../../src/staged/fixpoint_debt.txt");

/// The worklist pops a loop head may take.
const POPS: &[&str] = &[".pop()", ".pop_front()", ".pop_back()", ".pop_first()", ".pop_last()", ".iter().next()"];

/// The ways a driver puts work back on its worklist, after the worklist's name.
const REFILLS: &[&str] = &[".push(", ".push_back(", ".push_front(", ".insert("];

/// The names of a lattice join; a call of one of them, or of one of them followed by `_` and more.
const JOINS: &[&str] = &["meet", "join", "merge", "widen"];

fn ident_char(c: char) -> bool { c.is_ascii_alphanumeric() || c == '_' }

/// Whether `relative` (relative to `crates/`) is test code: a `tests` or `*_tests` module file or directory.
fn is_test_file(relative: &str) -> bool {
    relative.split('/').map(|part| part.strip_suffix(".rs").unwrap_or(part)).any(|part| part == "tests" || part.ends_with("_tests"))
}

/// The `.rs` files of `DRIVER_DIRECTORIES` that are not test code, relative to `crates/`.
fn source_files(crates: &Path) -> Vec<(String, PathBuf)> {
    let mut pending: Vec<PathBuf> = DRIVER_DIRECTORIES.iter().map(|directory| crates.join(directory)).collect();
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        let entries = std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("fixpoint scan directory {}: {error}", directory.display()));
        for entry in entries.map(Result::unwrap) {
            let path = entry.path();
            if path.is_dir() { pending.push(path); continue; }
            if path.extension().and_then(|extension| extension.to_str()) != Some("rs") { continue; }
            let relative = path.strip_prefix(crates).unwrap().to_string_lossy().replace('\\', "/");
            if !is_test_file(&relative) { files.push((relative, path)); }
        }
    }
    files.sort();
    files
}

/// The production code of `source`: without its inline test modules, wherever they stand, or comment lines.
fn production_code(source: &str) -> String {
    let code = super::semantics_duplication::without_test_modules(source);
    code.lines().filter(|line| !line.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n")
}

/// The byte ranges (from the opening brace to its match) of the bodies of the functions `code` defines.
fn function_bodies(code: &str) -> Vec<(usize, usize)> {
    let mut bodies = Vec::new();
    for (at, _) in code.match_indices("fn ") {
        if code[..at].ends_with(ident_char) { continue; }
        let Some(open) = code[at..].find(['{', ';']).map(|n| at + n) else { continue };
        if code.as_bytes()[open] == b';' { continue; }
        let mut depth = 0usize;
        for (offset, c) in code[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 { bodies.push((open, open + offset + 1)); break; }
                }
                _ => {}
            }
        }
    }
    bodies
}

/// The worklist a loop head at `at` pops (`while let Some(..) = W.pop()`), if the head is one.
fn popped_worklist(code: &str, at: usize) -> Option<&str> {
    let line = code[at..].split('\n').next().unwrap_or("");
    let (_, rest) = line.split_once(") = ")?;
    let expression = rest.split(" {").next().unwrap_or(rest).trim_end();
    let worklist = POPS.iter().find_map(|pop| expression.strip_suffix(pop))?;
    (!worklist.is_empty() && worklist.chars().all(|c| ident_char(c) || c == '.')).then_some(worklist)
}

/// Whether `code` calls a lattice join (`JOINS`): a name, as a function or a method, that is one of them
/// or starts with one of them and `_`, followed by `(`.
fn joins_states(code: &str) -> bool {
    JOINS.iter().any(|join| code.match_indices(join).any(|(at, _)| {
        if code[..at].ends_with(ident_char) { return false; }
        let rest = &code[at + join.len()..];
        let name_end = rest.find(|c: char| !ident_char(c)).unwrap_or(rest.len());
        let (suffix, after) = rest.split_at(name_end);
        // `strings.join(",")` joins text, not states.
        (suffix.is_empty() || suffix.starts_with('_')) && after.starts_with('(') && !after.starts_with("(\"")
    }))
}

/// The fixpoint drivers of `code` (the production code of one file): the worklist loop heads whose
/// innermost enclosing function refills the worklist after the head and joins states.
fn drivers(code: &str) -> usize {
    let bodies = function_bodies(code);
    code.match_indices("while let Some(").filter(|(at, _)| {
        if code[..*at].ends_with(ident_char) { return false; }
        let Some(worklist) = popped_worklist(code, *at) else { return false };
        let Some(&(_, end)) = bodies.iter().filter(|(open, end)| open < at && at < end).min_by_key(|(open, end)| end - open) else { return false };
        let after = &code[*at..end];
        REFILLS.iter().any(|refill| after.contains(&format!("{worklist}{refill}"))) && joins_states(after)
    }).count()
}

/// The fixpoint drivers of the tree outside `HOME`, per file.
fn found(crates: &Path) -> BTreeMap<String, usize> {
    let mut found = BTreeMap::new();
    for (relative, path) in source_files(crates) {
        if relative == HOME { continue; }
        let count = drivers(&production_code(&std::fs::read_to_string(&path).unwrap()));
        if count > 0 { found.insert(relative, count); }
    }
    found
}

fn render_debt(found: &BTreeMap<String, usize>) -> String {
    let total: usize = found.values().sum();
    let mut out = format!("# Fixpoint drivers outside `dataflow::solve` (`tests/staged_guards/fixpoint_drivers.rs`, which defines\n\
        # the shape): a worklist loop that refills its worklist and joins states. Lines are `file count`;\n\
        # counts may only shrink, and a new line fails. Goal: an empty file.\n\
        # total {total}\n");
    for (file, count) in found { out.push_str(&format!("{file} {count}\n")); }
    out
}

/// One fixpoint driver: a worklist loop beside `dataflow::solve` only goes away, and a new one fails.
#[test]
fn analyses_run_to_their_fixpoint_through_dataflow_solve() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let crates = manifest.parent().unwrap();
    let found = found(crates);
    if std::env::var_os("QUENCH_FIXPOINT_WRITE").is_some() {
        std::fs::write(manifest.join("src/staged/fixpoint_debt.txt"), render_debt(&found)).unwrap();
        return;
    }
    let allowed: BTreeMap<String, usize> = DEBT_FILE.lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let (file, count) = line.rsplit_once(' ').unwrap_or_else(|| panic!("malformed fixpoint debt line: {line}"));
            (file.to_owned(), count.parse().unwrap_or_else(|_| panic!("malformed fixpoint debt count: {line}")))
        })
        .collect();
    let grew: Vec<String> = found.iter().filter(|(file, count)| **count > allowed.get(*file).copied().unwrap_or(0))
        .map(|(file, count)| format!("{file}: {count} (allowed {})", allowed.get(file).copied().unwrap_or(0)))
        .collect();
    assert!(grew.is_empty(), "new fixpoint driver beside `dataflow::solve`; implement `dataflow::Dataflow` and call `solve` instead:\n{}",
        grew.join("\n"));
    assert!(found == allowed, "fixpoint drivers went away ({} -> {}): rerun with QUENCH_FIXPOINT_WRITE=1 to lower staged/fixpoint_debt.txt",
        allowed.values().sum::<usize>(), found.values().sum::<usize>());
}

#[test]
fn a_fixpoint_driver_refills_its_worklist_and_joins_states() {
    let driver = "fn prove() {\n    let mut pending = vec![0];\n    while let Some(k) = pending.pop() {\n        \
        let joined = meet(&a, &b);\n        if changed { pending.push(next); }\n    }\n}\n";
    let traversal = "fn reach() {\n    let mut pending = vec![0];\n    while let Some(k) = pending.pop() {\n        \
        if !seen[k] { seen[k] = true; pending.push(k + 1); }\n    }\n}\n";
    let drain = "fn drain() {\n    while let Some(k) = queue.pop_front() {\n        out.join_from(k);\n    }\n}\n";
    let bits = "fn solve() {\n    while let Some(block) = work.iter().next() {\n        \
        if analysis.join(known, &edge) { work.insert(target); }\n    }\n}\n";
    let widened = "fn plan() {\n    while let Some(k) = work.pop() {\n        let x = f.widened(p);\n        work.push(k);\n    }\n}\n";
    assert_eq!([driver, traversal, drain, bits, widened].map(drivers), [1, 0, 0, 1, 0]);
}
