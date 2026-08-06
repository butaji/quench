//! test262 staged runner — one stage at a time, 100% passing required.

mod collect;
mod digest;
pub mod execute;
pub mod flags;

use std::path::PathBuf;

use crate::harness::HarnessLoader;
use quench_runtime::host::{TestFailure, TestOutcome};

pub use execute::run_single_test;
pub use flags::default_stage;
use flags::RunnerFlags;

/// Absolute test262 root (`tests/test262`), for subprocess runners whose cwd may differ.
pub fn default_test262_dir() -> String {
    if let Ok(dir) = std::env::var("TEST262_DIR") {
        return dir;
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(&manifest);
    repo_root
        .join("tests/test262")
        .to_string_lossy()
        .into_owned()
}

/// Ordered stages (relative to test262/test/). The digest is authoritative;
/// `tasks/index.json` is descriptive configuration only.
pub const STAGES: &[&str] = &[
    "test/harness",
    "test/language/literals",
    "test/language/identifiers",
    "test/language/future-reserved-words",
    "test/language/reserved-words",
    "test/language/keywords",
    "test/language/punctuators",
    "test/language/white-space",
    "test/language/line-terminators",
    "test/language/comments",
    "test/language/source-text",
    "test/language/types",
    "test/language/directive-prologue",
    "test/language/statements/async-function",
    "test/language/statements/block",
    "test/language/statements/break",
    "test/language/statements/class",
    "test/language/statements/const",
    "test/language/statements/continue",
    "test/language/statements/debugger",
    "test/language/statements/do-while",
    "test/language/statements/empty",
    "test/language/statements/expression",
    "test/language/statements/for",
    "test/language/statements/for-in",
    "test/language/statements/for-of",
    "test/language/statements/function",
    "test/language/statements/generators",
    "test/language/statements/if",
    "test/language/statements/labeled",
    "test/language/statements/let",
    "test/language/statements/return",
    "test/language/statements/switch",
    "test/language/statements/throw",
    "test/language/statements/try",
    "test/language/statements/variable",
    "test/language/statements/while",
    "test/language/statements/with",
    "test/language/statements/async-generator",
    "test/language/statements/await-using",
    "test/language/statements/for-await-of",
    "test/language/statements/using",
    "test/language/statementList",
    "test/language/block-scope",
    "test/language/expressions",
    "test/language/computed-property-names",
    "test/language/destructuring",
    "test/language/rest-parameters",
    "test/language/function-code",
    "test/language/arguments-object",
    "test/language/eval-code",
    "test/language/global-code",
    "test/language/identifier-resolution",
    "test/language/module-code",
    "test/language/import",
    "test/language/export",
    "test/language/asi",
    "test/built-ins/global",
    "test/built-ins/Infinity",
    "test/built-ins/NaN",
    "test/built-ins/undefined",
    "test/built-ins/parseInt",
    "test/built-ins/parseFloat",
    "test/built-ins/isNaN",
    "test/built-ins/isFinite",
    "test/built-ins/decodeURI",
    "test/built-ins/decodeURIComponent",
    "test/built-ins/encodeURI",
    "test/built-ins/encodeURIComponent",
    "test/built-ins/eval",
    "test/built-ins/ThrowTypeError",
    "test/built-ins/Object",
    "test/built-ins/Function",
    "test/built-ins/Boolean",
    "test/built-ins/Error",
    "test/built-ins/NativeErrors",
    "test/built-ins/AggregateError",
    "test/built-ins/SuppressedError",
    "test/built-ins/Number",
    "test/built-ins/BigInt",
    "test/built-ins/Math",
    "test/built-ins/Date",
    "test/built-ins/String",
    "test/built-ins/Symbol",
    "test/built-ins/RegExp",
    "test/built-ins/Array",
    "test/built-ins/JSON",
    "test/built-ins/Iterator",
    "test/built-ins/ArrayIteratorPrototype",
    "test/built-ins/StringIteratorPrototype",
    "test/built-ins/RegExpStringIteratorPrototype",
    "test/built-ins/MapIteratorPrototype",
    "test/built-ins/SetIteratorPrototype",
    "test/built-ins/AsyncIteratorPrototype",
    "test/built-ins/AsyncFromSyncIteratorPrototype",
    "test/built-ins/GeneratorFunction",
    "test/built-ins/GeneratorPrototype",
    "test/built-ins/AsyncGeneratorFunction",
    "test/built-ins/AsyncGeneratorPrototype",
    "test/built-ins/AsyncFunction",
    "test/built-ins/ArrayBuffer",
    "test/built-ins/SharedArrayBuffer",
    "test/built-ins/TypedArray",
    "test/built-ins/TypedArrayConstructors",
    "test/built-ins/Uint8Array",
    "test/built-ins/DataView",
    "test/built-ins/Atomics",
    "test/built-ins/Map",
    "test/built-ins/Set",
    "test/built-ins/WeakMap",
    "test/built-ins/WeakSet",
    "test/built-ins/WeakRef",
    "test/built-ins/FinalizationRegistry",
    "test/built-ins/Promise",
    "test/built-ins/Reflect",
    "test/built-ins/Proxy",
    "test/built-ins/DisposableStack",
    "test/built-ins/AsyncDisposableStack",
    "test/built-ins/ShadowRealm",
    "test/built-ins/AbstractModuleSource",
    "test/built-ins/Temporal",
    "test/annexB",
];

#[derive(Debug, Default, Clone)]
pub struct RunSummary {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub first_failure: Option<(String, String)>,
}

pub struct Test262Runner {
    pub test262_dir: PathBuf,
    pub harness: HarnessLoader,
}

impl Test262Runner {
    pub fn new(test262_dir: PathBuf) -> Self {
        Self {
            harness: HarnessLoader::new(test262_dir.to_str().unwrap_or(".")),
            test262_dir,
        }
    }

    pub fn run(&self) -> RunSummary {
        let flags = RunnerFlags::from_env();
        let mut total = RunSummary::default();
        let mut stage = flags.stage;
        while let Some(stage_dir) = STAGES.get(stage).copied() {
            let s = if flags.digest {
                self.digest_stage(stage, stage_dir, &flags)
            } else {
                self.run_stage(stage, stage_dir, &flags)
            };
            total.passed += s.passed;
            total.failed += s.failed;
            total.skipped += s.skipped;
            if s.failed > 0 && !flags.digest {
                total.first_failure = s.first_failure;
                break;
            }
            if !flags.all_stages && !flags.digest {
                break;
            }
            // Digest defaults to one stage unless ALL_STAGES=1
            if flags.digest && !flags.all_stages {
                break;
            }
            stage += 1;
        }
        if flags.all_stages && total.failed == 0 && total.skipped == 0 {
            println!(
                "\n=== ALL STAGES COMPLETE — {} stages passed ===",
                STAGES.len()
            );
        }
        total
    }

    fn digest_stage(&self, stage: usize, stage_dir: &str, flags: &RunnerFlags) -> RunSummary {
        let full_path = self.test262_dir.join(stage_dir);
        if !full_path.exists() {
            return missing_stage_summary(stage_dir, &full_path);
        }
        let tests = collect::collect_tests(&full_path);
        digest::run_stage_digest(&self.harness, stage, stage_dir, &tests, flags).summary
    }

    fn run_stage(&self, stage: usize, stage_dir: &str, flags: &RunnerFlags) -> RunSummary {
        let full_path = self.test262_dir.join(stage_dir);
        if !full_path.exists() {
            return missing_stage_summary(stage_dir, &full_path);
        }
        let tests = collect::collect_tests(&full_path);
        let count = tests.len();
        if !flags.quick {
            println!("\n=== Stage {}: {} ({} tests) ===", stage, stage_dir, count);
        }
        let mut summary = RunSummary::default();
        for (i, path) in tests.iter().enumerate() {
            match run_single_test(&self.harness, path) {
                TestOutcome::Pass => {
                    summary.passed += 1;
                    if !flags.quick && summary.passed % 100 == 0 {
                        println!("  ... {} passed", summary.passed);
                    }
                }
                TestOutcome::Skip { reason } => {
                    summary.failed += 1;
                    summary.first_failure = Some((
                        path.display().to_string(),
                        format!("test was skipped: {}", reason),
                    ));
                    break;
                }
                TestOutcome::Fail { failure } => {
                    summary.failed += 1;
                    summary.first_failure =
                        Some((path.display().to_string(), failure.message.clone()));
                    print_rich_failure(stage, i, path, &failure);
                    break;
                }
            }
        }
        print_stage_footer(stage, count, &summary);
        summary
    }
}

fn print_rich_failure(stage: usize, i: usize, path: &std::path::Path, failure: &TestFailure) {
    println!(
        "\n============================================================\n\
         FIRST FAILURE\n\
         Stage {} | #{}\n\
         {}",
        stage,
        i,
        path.display()
    );
    // Error type and message
    if let Some(ref et) = failure.error_type {
        println!("  Type: {}", et);
    }
    println!("  Reason: {}", failure.message);
    if let Some(ref em) = failure.error_message {
        if Some(em) != failure.error_type.as_ref() {
            println!("  JS message: {}", em);
        }
    }
    // JS stack trace
    if let Some(ref stack) = failure.js_stack {
        println!("  Stack:");
        for line in stack.lines() {
            println!("    {}", line);
        }
    }
    // Source context (prefer detailed source_context, fall back to first 20 lines)
    if !failure.source_context.is_empty() {
        println!("  ── Source ──────────────────────────────────");
        println!("{}", failure.source_context);
    } else {
        let src_diag = std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .take(20)
            .collect::<Vec<_>>()
            .join("\n");
        if !src_diag.is_empty() {
            println!("  ── Source (first 20 lines) ────────────────");
            println!("{}", src_diag);
        }
    }
    println!("============================================================");
}

fn print_stage_footer(stage: usize, count: usize, summary: &RunSummary) {
    if summary.failed == 0 && summary.skipped > 0 {
        println!(
            "STAGE INCOMPLETE — Stage {}: {}/{} passed, {} skipped (skips block completion)",
            stage, summary.passed, count, summary.skipped
        );
    } else if summary.failed == 0 {
        println!(
            "ALL STAGES COMPLETE — Stage {}: {}/{} (skipped {})",
            stage, summary.passed, count, summary.skipped
        );
    } else {
        println!(
            "Stage {}: {}/{} passed, {} skipped (first failure reported)",
            stage, summary.passed, count, summary.skipped
        );
    }
}

/// A stage is complete only with zero failures and zero skips.
pub fn stage_is_complete(summary: &RunSummary) -> bool {
    summary.failed == 0 && summary.skipped == 0
}

/// Summary for a missing stage directory — a failure, never a silent pass.
fn missing_stage_summary(stage_dir: &str, full_path: &std::path::Path) -> RunSummary {
    println!("[MISSING] {}", full_path.display());
    RunSummary {
        failed: 1,
        first_failure: Some((
            stage_dir.to_string(),
            format!("missing stage directory: {}", full_path.display()),
        )),
        ..RunSummary::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_stage_dir_is_a_failure() {
        let s = missing_stage_summary("test/nope", std::path::Path::new("/x/test/nope"));
        assert_eq!(s.failed, 1);
        let (stage, reason) = s.first_failure.unwrap();
        assert_eq!(stage, "test/nope");
        assert!(reason.contains("missing stage directory"));
    }

    #[test]
    fn stage_with_skips_is_not_complete() {
        let s = RunSummary {
            passed: 10,
            skipped: 1,
            ..RunSummary::default()
        };
        assert!(!stage_is_complete(&s));
    }

    #[test]
    fn stage_without_failures_or_skips_is_complete() {
        let s = RunSummary {
            passed: 10,
            ..RunSummary::default()
        };
        assert!(stage_is_complete(&s));
    }
}
