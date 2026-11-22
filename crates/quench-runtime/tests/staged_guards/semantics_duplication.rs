//! The semantics-duplication test: a JS meaning written by hand in a tier, found by the source
//! signature it leaves rather than by an opcode name (`coverage_tests.rs` counts the names).

use std::path::{Path, PathBuf};

/// One semantic fingerprint: the meaning it identifies, the file that may implement it, and the
/// source signatures of a hand-written re-implementation.
struct Fingerprint {
    name: &'static str,
    /// Files (relative to `crates/`) that implement the meaning; a path ending in `/` covers a directory.
    home: &'static [&'static str],
    /// A line matches when it contains every literal of some group.
    groups: &'static [&'static [&'static str]],
}

/// The JS meanings the tiers must take from their staged home. Each group is a spelling that only a
/// hand-written implementation has: a call to the home (`math::int_multiply(...)`) matches none.
const FINGERPRINTS: &[Fingerprint] = &[
    Fingerprint {
        name: "to_uint32",
        home: &["quench-runtime/src/staged/math.rs"],
        groups: &[
            &["% 4294967296"],
            &["% 4_294_967_296"],
            &["rem_euclid(4294967296"],
            &["rem_euclid(4_294_967_296"],
            &["0x1_0000_0000"],
            &["powi(32)"],
            &["fn to_int32("],
            &["fn to_uint32("],
            &["fn to_u32("],
        ],
    },
    Fingerprint {
        name: "clz32",
        home: &["quench-runtime/src/staged/math.rs"],
        groups: &[
            &["leading_zeros()", "uint32"],
            &["leading_zeros()", "to_uint32"],
            &["fn clz32"],
        ],
    },
    Fingerprint {
        name: "imul",
        home: &["quench-runtime/src/staged/int_math.rs", "quench-runtime/src/staged/math.rs"],
        groups: &[
            &["wrapping_mul", "i32"],
            &["wrapping_mul", "int32"],
            &["Imul", "wrapping_mul"],
            &["fn imul("],
            &["fn int_multiply("],
        ],
    },
    Fingerprint {
        name: "canonical_nan",
        home: &["quench-runtime/src/staged/value_encoding.rs"],
        groups: &[
            &["0x7ff8"],
            &["0x7FF8"],
            &["NAN.to_bits()"],
            &["is_nan()", "CANONICAL_NAN"],
        ],
    },
    Fingerprint {
        name: "int32_tag",
        home: &["quench-runtime/src/staged/value_encoding.rs", "quench-runtime/src/staged/layout_offsets.rs"],
        groups: &[
            &["INT_TAG", "=="],
            &["INT_TAG", "!="],
            &["INT_TAG", "^"],
            &["TAG_INT", "=="],
            &["TAG_INT", "^"],
        ],
    },
];

/// The directories scanned, relative to `crates/`: the interpreter under `vm/`, the engine crate that
/// holds the lane and compiled tiers, and the b-jit stencils.
const TIER_DIRECTORIES: &[&str] = &["quench-runtime/src/vm", "quench-runtime-engine/src", "quench-bjit/stencils"];

/// The allowances: one `fingerprint path:text` line per hit that is allowed to stand.
const DEBT_FILE: &str = include_str!("../../src/staged/semantics_debt.txt");

/// A hit: a source line outside the home of its fingerprint.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Hit {
    fingerprint: &'static str,
    path: String,
    line: usize,
    text: String,
}

/// One allowance of the debt file, and whether a hit has used it.
struct Allowance {
    fingerprint: String,
    path: String,
    text: String,
    used: bool,
}

/// Whether `relative` (relative to `crates/`) is test code: a `tests` or `*_tests` module file or directory.
fn is_test_file(relative: &str) -> bool {
    relative.split('/').map(|part| part.strip_suffix(".rs").unwrap_or(part)).any(|part| part == "tests" || part.ends_with("_tests"))
}

/// The `.rs` files of the scanned directories that are not test code, sorted by path.
/// Each is returned with its path relative to `crates/`, so a home or allowance names it the same way.
fn source_files(crates: &Path) -> Vec<(String, PathBuf)> {
    let mut pending: Vec<PathBuf> = TIER_DIRECTORIES.iter().map(|directory| crates.join(directory)).collect();
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        let entries = std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("semantics scan directory {}: {error}", directory.display()));
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

/// Whether `text` contains every literal of some group of `fingerprint`.
fn matches(fingerprint: &Fingerprint, text: &str) -> bool {
    fingerprint.groups.iter().any(|group| group.iter().all(|literal| text.contains(literal)))
}

/// Whether `relative` is the home `home`, or lies in it when `home` names a directory.
fn in_home(home: &str, relative: &str) -> bool {
    if home.ends_with('/') { relative.starts_with(home) } else { relative == home }
}

/// Appends to `hits` every line of `source` (the file `relative`) that matches a fingerprint outside its
/// home. Inline test modules close the production code, as in `coverage_tests`. A signature split over two
/// lines counts once, at its first line; a comment line is never a hit.
fn scan(relative: &str, source: &str, hits: &mut Vec<Hit>) {
    let code = source.split("\n#[cfg(test)]\nmod ").next().unwrap_or(source);
    let code = code.split("#[cfg(test)]\nmod ").next().unwrap_or(code);
    let lines: Vec<&str> = code.lines().map(str::trim).collect();
    for (index, line) in lines.iter().enumerate() {
        if line.is_empty() || line.starts_with("//") { continue; }
        let next = lines.get(index + 1).copied().filter(|next| !next.starts_with("//"));
        for fingerprint in FINGERPRINTS {
            if fingerprint.home.iter().any(|home| in_home(home, relative)) { continue; }
            let spans = next.is_some_and(|next| !matches(fingerprint, next) && matches(fingerprint, &format!("{line} {next}")));
            if matches(fingerprint, line) || spans {
                hits.push(Hit { fingerprint: fingerprint.name, path: relative.to_owned(), line: index + 1, text: (*line).to_owned() });
            }
        }
    }
}

/// The debt file for `hits`: one `fingerprint path:text` line per hit.
fn render_debt(hits: &[Hit]) -> String {
    let mut out = String::from("# Semantic debt: each line allows one hit of a fingerprint (`tests/staged_guards/semantics_duplication.rs`),\n\
        # `fingerprint path:text`, where text is the trimmed source line. A hit outside its home that no line\n\
        # allows fails, and so does a line no hit uses. Lines may only go away. Goal: an empty file.\n");
    for hit in hits { out.push_str(&format!("{} {}:{}\n", hit.fingerprint, hit.path, hit.text)); }
    out
}

/// The allowances of `DEBT_FILE`, one per line that is not a comment.
fn allowances() -> Vec<Allowance> {
    DEBT_FILE.lines().filter(|line| !line.starts_with('#') && !line.trim().is_empty()).map(|line| {
        let (fingerprint, rest) = line.split_once(' ').unwrap_or_else(|| panic!("malformed semantic debt line: {line}"));
        let (path, text) = rest.split_once(':').unwrap_or_else(|| panic!("malformed semantic debt line: {line}"));
        Allowance { fingerprint: fingerprint.to_owned(), path: path.to_owned(), text: text.to_owned(), used: false }
    }).collect()
}

/// A JS meaning re-implemented by hand in a tier, by its source signature, is debt; a new one fails.
/// `QUENCH_SEMANTICS_WRITE=1` rewrites `staged/semantics_debt.txt` from the tree after a deliberate change.
#[test]
fn tiers_do_not_reimplement_js_semantics() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let crates = manifest.parent().unwrap();
    let mut hits = Vec::new();
    for (relative, path) in source_files(crates) {
        let source = std::fs::read_to_string(&path).unwrap();
        scan(&relative, &source, &mut hits);
    }
    hits.sort();
    if std::env::var_os("QUENCH_SEMANTICS_WRITE").is_some() {
        std::fs::write(manifest.join("src/staged/semantics_debt.txt"), render_debt(&hits)).unwrap();
        return;
    }
    let mut allowed = allowances();
    // An exact line claims its own allowance first, so a line that contains another allowed line cannot take it.
    let claim = |allowed: &mut Vec<Allowance>, hit: &Hit, exact: bool| {
        let fits = |a: &Allowance| !a.used && a.fingerprint == hit.fingerprint && a.path == hit.path
            && if exact { a.text == hit.text } else { hit.text.contains(a.text.as_str()) };
        allowed.iter_mut().find(|a| fits(a)).map(|a| a.used = true).is_some()
    };
    let mut pending: Vec<&Hit> = hits.iter().filter(|hit| !claim(&mut allowed, hit, true)).collect();
    pending.retain(|hit| !claim(&mut allowed, hit, false));
    let unallowed: Vec<String> = pending.iter().map(|hit| format!("{} {}:{}: {}", hit.fingerprint, hit.path, hit.line, hit.text)).collect();
    let stale: Vec<String> = allowed.iter().filter(|a| !a.used).map(|a| format!("{} {}:{}", a.fingerprint, a.path, a.text)).collect();
    assert!(unallowed.is_empty(),
        "hand-written JS semantics outside their home; call the staged home instead:\n{}", unallowed.join("\n"));
    assert!(stale.is_empty(),
        "semantic debt no longer present; remove these lines from staged/semantics_debt.txt (or rerun with QUENCH_SEMANTICS_WRITE=1):\n{}",
        stale.join("\n"));
}
