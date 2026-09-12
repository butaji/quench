//! Runner for the upstream WebAssembly specification testsuite.
//!
//! The testsuite lives in the `testsuite/` git submodule. This crate owns
//! filesystem discovery and reporting. Every `.wast` under the tree is walked,
//! including `proposals/`. Scoring is per directive via [`quench_wasm::Engine`].

use std::{
    fs,
    path::{Path, PathBuf},
};

pub use quench_wasm::{DirectiveResult, WastReport};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestFailure {
    pub path: PathBuf,
    pub line: usize,
    pub directive: String,
    pub expected: String,
    pub got: String,
}

impl TestFailure {
    pub fn format_line(&self) -> String {
        format!(
            "{}:{} {}: expected {}; got {}",
            self.path.display(),
            self.line,
            self.directive,
            self.expected,
            self.got
        )
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct TestReport {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub failures: Vec<TestFailure>,
}

pub struct TestSuite {
    root: PathBuf,
    engine: quench_wasm::Engine,
}

impl TestSuite {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            engine: quench_wasm::Engine::new(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn files(&self) -> impl Iterator<Item = PathBuf> {
        let mut files = walkdir::WalkDir::new(&self.root)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "wast"))
            .map(|entry| entry.into_path())
            .collect::<Vec<_>>();
        files.sort();
        files.into_iter()
    }

    pub fn run_file(&self, path: impl AsRef<Path>) -> WastReport {
        let path = path.as_ref();
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) => {
                return WastReport {
                    results: vec![DirectiveResult {
                        line: 1,
                        kind: "wast".to_string(),
                        passed: false,
                        expected: "read".to_string(),
                        got: error.to_string(),
                    }],
                };
            }
        };
        let filename = path.to_string_lossy();
        self.engine.run_wast(&filename, &source)
    }

    pub fn run_all(&self) -> TestReport {
        let mut report = TestReport::default();
        for path in self.files() {
            let file_report = self.run_file(&path);
            for result in file_report.results {
                report.total += 1;
                if result.passed {
                    report.passed += 1;
                } else {
                    report.failed += 1;
                    report.failures.push(TestFailure {
                        path: path.clone(),
                        line: result.line,
                        directive: result.kind,
                        expected: result.expected,
                        got: result.got,
                    });
                }
            }
        }
        report
    }
}

pub fn testsuite_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testsuite")
}

#[cfg(test)]
mod tests {
    use super::{testsuite_root, TestSuite};

    #[test]
    fn scores_fixture_directives_not_files() {
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("smoke.wast");
        std::fs::write(
            &path,
            r#"
(assert_malformed (module binary "") "unexpected end")
(assert_invalid
  (module (func (unreachable) (drop (local.get 0))))
  "unknown local")
(module (func (export "answer") (result i32) i32.const 42))
(assert_return (invoke "answer") (i32.const 42))
(invoke "answer")
"#,
        )
        .expect("write");
        let suite = TestSuite::new(root.path());
        let report = suite.run_all();
        assert_eq!(report.total, 5, "{report:?}");
        assert_eq!(report.passed, 5, "{report:?}");
        assert_eq!(report.failed, 0, "{report:?}");
    }

    #[test]
    fn vendored_unreached_invalid_validator_directives_pass() {
        let path = testsuite_root().join("unreached-invalid.wast");
        let report = TestSuite::new(testsuite_root()).run_file(&path);
        assert!(
            !report.results.is_empty(),
            "unreached-invalid.wast produced no directives"
        );
        let failed: Vec<_> = report
            .results
            .iter()
            .filter(|r| !r.passed)
            .map(|r| r.format_line("unreached-invalid.wast"))
            .collect();
        assert!(
            failed.is_empty(),
            "validator file had failures:\n{}",
            failed.join("\n")
        );
        assert!(report.results.iter().all(|r| r.kind == "assert_invalid"));
    }

    #[test]
    fn vendored_binary_malformed_passes_and_modules_validate() {
        let path = testsuite_root().join("binary.wast");
        let report = TestSuite::new(testsuite_root()).run_file(&path);
        assert!(!report.results.is_empty());
        let validator_failed: Vec<_> = report
            .results
            .iter()
            .filter(|r| {
                matches!(
                    r.kind.as_str(),
                    "assert_malformed" | "assert_invalid" | "module"
                ) && !r.passed
            })
            .map(|r| r.format_line("binary.wast"))
            .collect();
        assert!(
            validator_failed.is_empty(),
            "binary.wast validator failures:\n{}",
            validator_failed.join("\n")
        );
    }

    #[test]
    fn vendored_numeric_and_control_execute_passes() {
        let root = testsuite_root();
        let suite = TestSuite::new(&root);
        let mut failed = Vec::new();
        for rel in [
            "i32.wast",
            "i64.wast",
            "f32.wast",
            "f64.wast",
            "const.wast",
            "local_get.wast",
            "local_set.wast",
            "local_tee.wast",
            "select.wast",
            "block.wast",
            "loop.wast",
            "if.wast",
            "br.wast",
            "br_if.wast",
            "call.wast",
            "return.wast",
            "nop.wast",
            "unreachable.wast",
        ] {
            let report = suite.run_file(root.join(rel));
            for result in &report.results {
                if !result.passed {
                    failed.push(result.format_line(rel));
                }
            }
        }
        assert!(
            failed.is_empty(),
            "{} numeric/control failures:\n{}",
            failed.len(),
            failed
                .iter()
                .take(40)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    #[test]
    fn vendored_memory_table_global_execute() {
        let root = testsuite_root();
        let suite = TestSuite::new(&root);
        let mut failed = Vec::new();
        for rel in [
            "memory.wast",
            "memory_size.wast",
            "memory_grow.wast",
            "load.wast",
            "store.wast",
            "global.wast",
            "start.wast",
            "bulk.wast",
            "table.wast",
            "table_get.wast",
            "table_set.wast",
            "table_size.wast",
            "table_grow.wast",
            "table_fill.wast",
            "table_copy.wast",
            "table_init.wast",
            "memory_copy.wast",
            "memory_fill.wast",
            "memory_init.wast",
            "exports.wast",
            "imports.wast",
            "linking.wast",
            "elem.wast",
            "data.wast",
            "func_ptrs.wast",
            "call_indirect.wast",
            "ref_func.wast",
            "ref_is_null.wast",
            "ref_null.wast",
        ] {
            let report = suite.run_file(root.join(rel));
            for result in &report.results {
                if !result.passed {
                    failed.push(result.format_line(rel));
                }
            }
        }
        assert!(
            failed.is_empty(),
            "{} memory/table/link failures:\n{}",
            failed.len(),
            failed
                .iter()
                .take(50)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    #[test]
    fn vendored_bulk_refs_simd_execute() {
        let root = testsuite_root();
        let suite = TestSuite::new(&root);
        let mut failed = Vec::new();
        for rel in [
            "return_call.wast",
            "return_call_indirect.wast",
            "ref.wast",
            "ref_as_non_null.wast",
            "simd_const.wast",
            "simd_splat.wast",
            "simd_bitwise.wast",
            "simd_boolean.wast",
            "simd_i8x16_arith.wast",
            "simd_i16x8_arith.wast",
            "simd_i32x4_arith.wast",
            "simd_i64x2_arith.wast",
            "simd_f32x4_arith.wast",
            "simd_f64x2_arith.wast",
            "simd_lane.wast",
            "simd_load.wast",
            "simd_store.wast",
            "simd_select.wast",
            "simd_bit_shift.wast",
            "simd_i8x16_sat_arith.wast",
            "simd_i16x8_sat_arith.wast",
            "simd_i8x16_cmp.wast",
            "simd_i16x8_cmp.wast",
            "simd_i32x4_cmp.wast",
            "simd_i64x2_cmp.wast",
            "simd_f32x4_cmp.wast",
            "simd_f64x2_cmp.wast",
            "simd_load_splat.wast",
            "simd_load_zero.wast",
            "simd_load_extend.wast",
            "simd_conversions.wast",
            "simd_int_to_int_extend.wast",
            "simd_i8x16_arith2.wast",
            "simd_i16x8_arith2.wast",
            "simd_i32x4_arith2.wast",
            "simd_i64x2_arith2.wast",
            "simd_load8_lane.wast",
            "simd_store8_lane.wast",
            "simd_f32x4_rounding.wast",
            "simd_f32x4_pmin_pmax.wast",
            "simd_f64x2_rounding.wast",
            "simd_f64x2_pmin_pmax.wast",
            "simd_i16x8_extadd_pairwise_i8x16.wast",
            "simd_i16x8_extmul_i8x16.wast",
            "simd_i16x8_q15mulr_sat_s.wast",
            "simd_i32x4_dot_i16x8.wast",
            "simd_i32x4_extadd_pairwise_i16x8.wast",
            "simd_i32x4_extmul_i16x8.wast",
            "simd_i32x4_trunc_sat_f32x4.wast",
            "simd_i32x4_trunc_sat_f64x2.wast",
            "simd_i64x2_extmul_i32x4.wast",
            "simd_load16_lane.wast",
            "simd_load32_lane.wast",
            "simd_load64_lane.wast",
            "simd_store16_lane.wast",
            "simd_store32_lane.wast",
            "simd_store64_lane.wast",
            "relaxed_dot_product.wast",
            "relaxed_madd_nmadd.wast",
        ] {
            let report = suite.run_file(root.join(rel));
            for result in &report.results {
                if !result.passed {
                    failed.push(result.format_line(rel));
                }
            }
        }
        let mut by_file = std::collections::BTreeMap::<&str, usize>::new();
        for line in &failed {
            if let Some(name) = line.split(':').next() {
                *by_file.entry(name).or_insert(0) += 1;
            }
        }
        let summary = by_file
            .iter()
            .map(|(f, n)| format!("{n} {f}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            failed.is_empty(),
            "{} bulk/refs/simd failures:\n{}\n{}",
            failed.len(),
            summary,
            failed
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    #[test]
    fn vendored_proposals_mid_execute() {
        let root = testsuite_root();
        let suite = TestSuite::new(&root);
        let mut failed = Vec::new();
        for rel in [
            "memory64.wast",
            "memory-multi.wast",
            "proposals/wide-arithmetic/wide-arithmetic.wast",
            "proposals/custom-page-sizes/custom-page-sizes.wast",
            "proposals/threads/atomic.wast",
            "proposals/threads/memory.wast",
            "proposals/threads/imports.wast",
            "proposals/threads/exports.wast",
        ] {
            let report = suite.run_file(root.join(rel));
            for result in &report.results {
                if !result.passed {
                    failed.push(result.format_line(rel));
                }
            }
        }
        let mut by_file = std::collections::BTreeMap::<&str, usize>::new();
        for line in &failed {
            if let Some(name) = line.split(':').next() {
                *by_file.entry(name).or_insert(0) += 1;
            }
        }
        let summary = by_file
            .iter()
            .map(|(f, n)| format!("{n} {f}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            failed.is_empty(),
            "{} proposals-mid failures:\n{}\n{}",
            failed.len(),
            summary,
            failed
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    #[test]
    fn vendored_gc_exn_execute() {
        let root = testsuite_root();
        let suite = TestSuite::new(&root);
        let mut failed = Vec::new();
        for rel in [
            "i31.wast",
            "ref_cast.wast",
            "ref_test.wast",
            "ref_eq.wast",
            "array.wast",
            "array_fill.wast",
            "array_copy.wast",
            "array_new_data.wast",
            "array_new_elem.wast",
            "array_init_data.wast",
            "array_init_elem.wast",
            "struct.wast",
            "br_on_null.wast",
            "br_on_non_null.wast",
            "br_on_cast.wast",
            "br_on_cast_fail.wast",
            "call_ref.wast",
            "return_call_ref.wast",
            "throw.wast",
            "throw_ref.wast",
            "try_table.wast",
            "tag.wast",
            "extern.wast",
            "legacy/try_catch.wast",
            "legacy/throw.wast",
            "legacy/rethrow.wast",
            "legacy/try_delegate.wast",
            "type-subtyping.wast",
            "linking0.wast",
        ] {
            let report = suite.run_file(root.join(rel));
            for result in &report.results {
                if !result.passed {
                    failed.push(result.format_line(rel));
                }
            }
        }
        assert!(
            failed.is_empty(),
            "{} gc/exn failures:\n{}",
            failed.len(),
            failed
                .iter()
                .take(30)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    #[test]
    fn vendored_custom_descriptors_execute() {
        let root = testsuite_root();
        let suite = TestSuite::new(&root);
        let mut failed = Vec::new();
        for rel in [
            "proposals/custom-descriptors/struct_new_desc.wast",
            "proposals/custom-descriptors/ref_get_desc.wast",
            "proposals/custom-descriptors/ref_cast_desc_eq.wast",
            "proposals/custom-descriptors/br_on_cast_desc_eq.wast",
            "proposals/custom-descriptors/br_on_cast_desc_eq_fail.wast",
            "proposals/custom-descriptors/exact-casts.wast",
            "proposals/custom-descriptors/exact.wast",
            "proposals/custom-descriptors/descriptors.wast",
            "proposals/custom-descriptors/array_new_exact.wast",
            "proposals/custom-descriptors/br_on_cast.wast",
            "proposals/custom-descriptors/br_on_cast_fail.wast",
            "proposals/custom-descriptors/exact-func-import.wast",
            "proposals/custom-descriptors/binary.wast",
            "proposals/custom-descriptors/binary-descriptors.wast",
        ] {
            let report = suite.run_file(root.join(rel));
            for result in &report.results {
                if !result.passed {
                    failed.push(result.format_line(rel));
                }
            }
        }
        assert!(
            failed.is_empty(),
            "{} custom-descriptors failures:\n{}",
            failed.len(),
            failed
                .iter()
                .take(40)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    #[test]
    fn vendored_i32_execute_passes() {
        let path = testsuite_root().join("i32.wast");
        let report = TestSuite::new(testsuite_root()).run_file(&path);
        let failed: Vec<_> = report
            .results
            .iter()
            .filter(|r| !r.passed)
            .map(|r| r.format_line("i32.wast"))
            .collect();
        assert!(
            failed.is_empty(),
            "{} i32.wast failures:\n{}",
            failed.len(),
            failed.join("\n")
        );
    }

    fn is_validator_kind(kind: &str) -> bool {
        matches!(
            kind,
            "assert_malformed" | "assert_invalid" | "module" | "wast"
        )
    }

    #[test]
    fn every_vendored_validator_and_module_directive_passes() {
        let root = testsuite_root();
        let suite = TestSuite::new(&root);
        let mut scanned = 0usize;
        let mut failures = Vec::new();
        for path in suite.files() {
            let report = suite.run_file(&path);
            for result in &report.results {
                if !is_validator_kind(&result.kind) {
                    continue;
                }
                scanned += 1;
                if !result.passed {
                    failures.push(result.format_line(&path.to_string_lossy()));
                }
            }
        }
        assert!(
            scanned > 0,
            "walked no validator/module directives under {}",
            root.display()
        );
        assert!(
            failures.is_empty(),
            "{} validator/module failures (scanned {scanned}):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn names_and_legacy_are_scored_as_directives() {
        let root = testsuite_root();
        let suite = TestSuite::new(&root);
        for rel in [
            "names.wast",
            "legacy/try_catch.wast",
            "legacy/throw.wast",
            "legacy/rethrow.wast",
            "legacy/try_delegate.wast",
        ] {
            let report = suite.run_file(root.join(rel));
            assert!(
                report.results.len() > 1,
                "{rel} should parse into directives, got {:?}",
                report.results
            );
            assert!(
                report.results.iter().all(|r| r.kind != "wast"),
                "{rel} was a single wast-parse failure: {:?}",
                report.results
            );
        }
    }

    #[test]
    fn proposals_are_walked_not_omitted() {
        let root = testsuite_root();
        let suite = TestSuite::new(&root);
        let proposals: Vec<_> = suite
            .files()
            .filter(|p| p.starts_with(root.join("proposals")))
            .collect();
        assert!(
            !proposals.is_empty(),
            "no proposals/ wast files discovered under {}",
            root.display()
        );
        let sample = root.join("proposals/wide-arithmetic/wide-arithmetic.wast");
        assert!(
            proposals.contains(&sample),
            "missing {sample:?} in {proposals:?}"
        );
        let report = suite.run_file(&sample);
        assert!(!report.results.is_empty());
        assert!(report
            .results
            .iter()
            .any(|r| r.kind == "module" && r.passed));
        assert!(
            report
                .results
                .iter()
                .any(|r| r.kind == "assert_return" && r.passed),
            "wide-arithmetic execute should pass on the shipped path"
        );
    }
}
