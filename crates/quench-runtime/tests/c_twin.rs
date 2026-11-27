//! S1.17 (TOD-2193): every staged row has a C twin. Each row's twin, from the staged `Operation`
//! (`staged::c_twins`) and the b-jit catalog's heap rows and Number kinds (`row_catalog.rs`), is either a
//! kernel the C corpus registers (`quench-bench/primitives/ops.c`, `{"name", op_name}`) or `None` with a reason.
//! That every row declares one is the compiler's: the matches have no catch-all and the catalog grammar
//! requires the `twin:` field. The 3C+6 bound is step 3: `quench-gate --only ir-budget` (section `c-bound`)
//! reads the rows and twins from `crates/quench-tier-corpus/c_twin_rows.txt`, which `the_rows_file_is_the_code`
//! holds to the code (`QUENCH_C_TWIN_WRITE=1` rewrites it), so the gate needs no runtime dependency.

use std::collections::BTreeSet;

use quench_runtime::staged::c_twins::{NumberOperationCTwinsExt, OperationCTwinsExt};
use quench_runtime::staged::{numeric::NumberOperation, CKernel, CTwin, Operation};

/// The kernel names `ops.c` registers in its table of measured ops.
fn corpus_kernels() -> BTreeSet<String> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quench-bench/primitives/ops.c");
    let source = std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{path}: {error}"));
    let mut names = BTreeSet::new();
    for entry in source.split("{\"").skip(1) {
        if let Some((name, rest)) = entry.split_once("\", op_") {
            if rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
                names.insert(name.to_string());
            }
        }
    }
    names
}

/// Every staged row by family and name, with its twin.
fn rows() -> Vec<(&'static str, &'static str, CTwin)> {
    let operations = Operation::<u64, ()>::c_twin_rows().into_iter().map(|(name, twin)| ("Operation", name, twin));
    let heap = quench_bjit::c_twin::HEAP_ROW_TWINS.iter().flat_map(|chunk| chunk.iter()).map(|&(name, twin)| ("heap row", name, twin));
    let number = quench_bjit::c_twin::NUMBER_ROW_TWINS.iter().map(|&(name, twin)| ("number kind", name, twin));
    operations.chain(heap).chain(number).collect()
}

fn check(family: &str, name: &str, twin: CTwin, corpus: &BTreeSet<String>) -> Option<String> {
    match twin {
        CTwin::Kernel(kernel) if !corpus.contains(kernel.name()) => Some(format!("{family} {name}: kernel {} is not in ops.c", kernel.name())),
        CTwin::None(reason) if reason.trim().is_empty() => Some(format!("{family} {name}: CTwin::None without a reason")),
        _ => None,
    }
}

#[test]
fn every_staged_row_has_a_c_twin() {
    let corpus = corpus_kernels();
    assert!(corpus.len() > 100, "ops.c registry not found or unparsed ({} names)", corpus.len());
    let rows = rows();
    let failures: Vec<String> = rows.iter().filter_map(|&(family, name, twin)| check(family, name, twin, &corpus)).collect();
    assert!(failures.is_empty(), "rows without a valid C twin:\n{}", failures.join("\n"));
    let kernels = rows.iter().filter(|(_, _, twin)| matches!(twin, CTwin::Kernel(_))).count();
    println!("{} staged rows: {kernels} with a C kernel, {} CTwin::None", rows.len(), rows.len() - kernels);
}

#[test]
fn every_named_kernel_is_in_the_corpus() {
    let corpus = corpus_kernels();
    let missing: Vec<&str> = CKernel::ALL.iter().map(|kernel| kernel.name()).filter(|name| !corpus.contains(*name)).collect();
    assert!(missing.is_empty(), "CKernel names not registered in ops.c: {missing:?}");
}

#[test]
fn row_names_are_unique_per_family() {
    let rows = rows();
    let mut seen = BTreeSet::new();
    let duplicates: Vec<_> = rows.iter().filter(|(family, name, _)| !seen.insert((*family, *name))).collect();
    assert!(duplicates.is_empty(), "duplicate rows: {duplicates:?}");
}

#[test]
fn number_operation_rows_are_the_number_variant() {
    // `Operation::Number` delegates to `NumberOperation::c_twin`, so its rows are the kinds' own.
    let kinds = NumberOperation::c_twin_rows();
    let operation_rows = Operation::<u64, ()>::c_twin_rows();
    for kind in &kinds {
        assert!(operation_rows.contains(kind), "Operation rows miss NumberOperation::{}", kind.0);
    }
}

/// The rows file the gate's `c-bound` section reads.
const ROWS_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../quench-tier-corpus/c_twin_rows.txt");

/// The rows as the gate reads them: `row kernel` or `row - reason`, the row prefixed by its family.
fn rows_file() -> String {
    let header = "# Every staged row and its C twin (S1.17, TOD-2193): `row kernel`, or `row - reason` where no ops.c kernel isolates it.\n\
                  # Generated from the code: QUENCH_C_TWIN_WRITE=1 cargo test -p quench-runtime --test c_twin. Read by quench-gate's\n\
                  # ir-budget step (section c-bound: default Ir/iter <= 3 * C + 6, held by c_bound_allowance.txt). Never edit by hand.\n";
    let body: String = rows().into_iter().map(|(family, name, twin)| {
        let row = match family {
            "heap row" => format!("heap::{name}"),
            "number kind" => format!("number::{name}"),
            _ => name.to_string(),
        };
        match twin {
            CTwin::Kernel(kernel) => format!("{row} {}\n", kernel.name()),
            CTwin::None(reason) => format!("{row} - {reason}\n"),
        }
    }).collect();
    format!("{header}{body}")
}

#[test]
fn the_rows_file_is_the_code() {
    let expected = rows_file();
    if std::env::var("QUENCH_C_TWIN_WRITE").as_deref() == Ok("1") {
        std::fs::write(ROWS_FILE, &expected).unwrap_or_else(|error| panic!("{ROWS_FILE}: {error}"));
    }
    let actual = std::fs::read_to_string(ROWS_FILE).unwrap_or_default();
    assert!(actual == expected, "{ROWS_FILE} is not the code's rows; rewrite it: QUENCH_C_TWIN_WRITE=1 cargo test -p quench-runtime --test c_twin");
}
