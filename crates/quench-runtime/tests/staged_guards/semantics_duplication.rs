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
    /// Directories (relative to `crates/`) scanned for this fingerprint alone, beyond `TIER_DIRECTORIES`:
    /// a meaning a host crate re-implements, whose other fingerprints would not apply there.
    also: &'static [&'static str],
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
        also: &[],
    },
    Fingerprint {
        name: "clz32",
        home: &["quench-runtime/src/staged/math.rs"],
        groups: &[
            &["leading_zeros()", "uint32"],
            &["leading_zeros()", "to_uint32"],
            &["fn clz32"],
        ],
        also: &[],
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
        also: &[],
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
        also: &[],
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
        also: &[],
    },
    // SSOT 1 (TOD-2231): `===`/`==` decided by hand beside the staged `generic::decide` row: the
    // identical-but-not-NaN test, the `null == undefined` pair, the short-string record compare, and
    // the bodies that held them (now thin delegators to the staged rows, so no longer fingerprinted).
    Fingerprint {
        name: "equality",
        home: &["quench-runtime/src/staged/generic.rs"],
        groups: &[
            &["Relation::NotEqual", "CANONICAL_NAN"],
            &["Binary::And, left_nullish, right_nullish"],
            &["TAG_SHORT", "Relation::Equal"],
        ],
        also: &[],
    },
    // SSOT 3 (TOD-2233): `++`/`--` of a Number by hand (an int32 `checked_add(1)`, a binary64 on
    // overflow) beside the staged `numeric::increment` row.
    Fingerprint {
        name: "increment",
        home: &["quench-runtime/src/staged/numeric.rs"],
        groups: &[
            &["integer.checked_add(1)"],
            &["integer.checked_sub(1)"],
            &["fn increment_delta("],
            &["fn number_increment("],
        ],
        also: &[],
    },
    // SSOT 3: `Math.sqrt` of a Number word computed in place beside `FloatUnary::Sqrt`
    // (`math::float_primitive`).
    Fingerprint {
        name: "sqrt",
        home: &["quench-runtime/src/staged/math.rs"],
        groups: &[
            &[".sqrt().to_bits()"],
            &["f64::sqrt("],
        ],
        also: &[],
    },
    // SSOT 3: ToInt32 of an integral Number by the 1.5 * 2^52 (2^52 + 2^51) rounding constant, beside
    // the staged `value::exact_int32`.
    Fingerprint {
        name: "exact_int32",
        home: &["quench-runtime/src/staged/value.rs"],
        groups: &[
            &["6755399441055744"],
            &["6_755_399_441_055_744"],
            &["fn exact_int32("],
        ],
        also: &[],
    },
    // SSOT 3: `charCodeAt` by hand (its own ToIntegerOrInfinity of the index, bounds test or unit read)
    // beside the staged `StringNative::CharCodeAt` row (`strings::concrete_char_code_at`, `builtins`).
    Fingerprint {
        name: "char_code_at",
        home: &["quench-runtime/src/staged/strings.rs", "quench-runtime/src/staged/builtins.rs"],
        groups: &[
            &["fn string_unit_index("],
            &["Value::number(f64::NAN)", "Value::number(unit"],
            &["index as u64 >= string.length"],
        ],
        also: &[],
    },
    // SSOT 3: NaN-box tag tests and int boxing written on the word (`x - FIRST_TAG >= TAG_SPAN`,
    // `v as u32 | INT_TAG`) beside the staged `value::is_double` and `value::int_word`.
    Fingerprint {
        name: "nan_box_tag",
        home: &["quench-runtime/src/staged/value.rs", "quench-runtime/src/staged/value_encoding.rs"],
        groups: &[
            &["wrapping_sub(FIRST_TAG)"],
            &["wrapping_sub(words::FIRST_TAG"],
            &["as u32) | INT_TAG"],
            &["fn is_double("],
            &["fn int_word("],
        ],
        also: &[],
    },
    // SSOT 3: ToUint8Clamp by another formula (clamp to `[0, u8::MAX]`, then round) beside the staged
    // `math::clamp_u8` that Concrete and the stencils run.
    Fingerprint {
        name: "clamp_u8",
        home: &["quench-runtime/src/staged/math.rs"],
        groups: &[
            &["f64::from(u8::MAX)"],
            &["u8::MAX as f64"],
            &["clamp(0.0, 255.0)"],
        ],
        also: &[],
    },
    // SSOT 3: int32 `+`/`-` of a Number operator wrapped by hand (a loop's closed forms) beside the
    // staged int32 arithmetic.
    Fingerprint {
        name: "int32_wrapping_arith",
        home: &["quench-runtime/src/staged/int_math.rs", "quench-runtime/src/staged/numeric.rs"],
        groups: &[
            &["NumberArithmetic::", "wrapping_add"],
            &["NumberArithmetic::", "wrapping_sub"],
        ],
        also: &[],
    },
    // SSOT 3: operator algebra (the bitwise family, commutation, the neutral operand, the float form)
    // written per tier instead of read from the rows. The b-jit's code table (`stencils/codes.rs`) is
    // the one copy kept until a staged home exists.
    Fingerprint {
        name: "op_algebra",
        home: &["quench-runtime/src/staged/", "quench-bjit/stencils/codes.rs"],
        groups: &[
            &["fn is_bitwise("],
            &["fn commutes("],
            &["fn neutral("],
            &["fn has_float_form("],
        ],
        also: &[],
    },
    // SSOT 15 (TOD-2244): ECMAScript WhiteSpace taken as Rust's `char::is_whitespace` plus U+FEFF, which
    // admits U+0085 and other spaces JS does not, beside the runtime's `is_ecma_whitespace_character`.
    // Scanned in the host crate too, whose `parseInt` trims this way.
    Fingerprint {
        name: "ecma_whitespace",
        home: &["quench-runtime/src/vm/primitives/whitespace.rs"],
        groups: &[
            &[".is_whitespace()", "'\\u{feff}'"],
            &[".is_whitespace()", "'\\u{FEFF}'"],
        ],
        also: &["quench/src"],
    },
];


/// The directories scanned, relative to `crates/`: the interpreter under `vm/`, the engine crate that
/// holds the lane and compiled tiers, and the b-jit (its planner under `src/` and its stencils).
const TIER_DIRECTORIES: &[&str] = &["quench-runtime/src/vm", "quench-runtime-engine/src", "quench-bjit/src", "quench-bjit/stencils"];

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

/// The `.rs` files of the scanned directories (`TIER_DIRECTORIES` and every fingerprint's `also`) that
/// are not test code, sorted by path. Each is returned with its path relative to `crates/`, so a home
/// or allowance names it the same way.
fn source_files(crates: &Path) -> Vec<(String, PathBuf)> {
    let mut directories: Vec<&str> = TIER_DIRECTORIES.to_vec();
    directories.extend(FINGERPRINTS.iter().flat_map(|fingerprint| fingerprint.also.iter().copied()));
    directories.sort_unstable();
    directories.dedup();
    let mut pending: Vec<PathBuf> = directories.iter().map(|directory| crates.join(directory)).collect();
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

/// Whether `fingerprint` scans `relative`: a file of `TIER_DIRECTORIES`, or of one of its own `also` directories.
fn in_scope(fingerprint: &Fingerprint, relative: &str) -> bool {
    let under = |directory: &&str| relative.starts_with(&format!("{directory}/"));
    TIER_DIRECTORIES.iter().any(under) || fingerprint.also.iter().any(under)
}

/// Whether `relative` is the home `home`, or lies in it when `home` names a directory.
fn in_home(home: &str, relative: &str) -> bool {
    if home.ends_with('/') { relative.starts_with(home) } else { relative == home }
}

/// `source` with each inline test module (`#[cfg(test)]` over `mod name {`) blanked to its line breaks, so
/// the production code after a test module in mid-file is still scanned and every line keeps its number.
pub(crate) fn without_test_modules(source: &str) -> String {
    const TEST: &str = "#[cfg(test)]\nmod ";
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(at) = rest.find(TEST) {
        out.push_str(&rest[..at]);
        let module = &rest[at..];
        // A `mod name;` declaration has no body here: keep it and go on.
        let Some(open) = module.find(['{', ';']).filter(|&open| module.as_bytes()[open] == b'{') else {
            out.push_str(TEST);
            rest = &module[TEST.len()..];
            continue;
        };
        let mut depth = 0usize;
        let end = module[open..].char_indices().find_map(|(offset, c)| {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 { return Some(open + offset + 1); }
                }
                _ => {}
            }
            None
        }).unwrap_or(module.len());
        out.extend(module[..end].chars().filter(|&c| c == '\n'));
        rest = &module[end..];
    }
    out.push_str(rest);
    out
}

#[test]
fn test_modules_are_left_out_wherever_they_stand() {
    let source = "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn t() { x }\n}\nfn b() {}\n#[cfg(test)]\nmod more;\nfn c() {}\n";
    let code = without_test_modules(source);
    assert_eq!(code.lines().count(), source.lines().count());
    assert!(code.contains("fn b()") && code.contains("fn c()") && !code.contains("fn t()"));
}

/// Appends to `hits` every line of `source` (the file `relative`) that matches a fingerprint outside its
/// home. Inline test modules are left out wherever they stand (`without_test_modules`). A signature split
/// over two lines counts once, at its first line; a comment line is never a hit.
fn scan(relative: &str, source: &str, hits: &mut Vec<Hit>) {
    let code = without_test_modules(source);
    let lines: Vec<&str> = code.lines().map(str::trim).collect();
    for (index, line) in lines.iter().enumerate() {
        if line.is_empty() || line.starts_with("//") { continue; }
        let next = lines.get(index + 1).copied().filter(|next| !next.starts_with("//"));
        for fingerprint in FINGERPRINTS {
            if !in_scope(fingerprint, relative) || fingerprint.home.iter().any(|home| in_home(home, relative)) { continue; }
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
