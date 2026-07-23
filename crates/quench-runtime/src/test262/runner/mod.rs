//! test262 staged runner — one stage at a time, 100% passing required.

mod collect;
mod digest;
mod execute;
mod flags;

use std::path::PathBuf;

use crate::test262::harness::HarnessLoader;
use crate::test262::host::{Test262Host, TestOutcome};

pub use execute::run_single_test;
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

/// Ordered stages (relative to test262/test/). Mirrors `tasks/index.json`.
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

    pub fn run(&self, host: &mut dyn Test262Host) -> RunSummary {
        let flags = RunnerFlags::from_env();
        let mut total = RunSummary::default();
        let mut stage = flags.stage;
        while let Some(stage_dir) = STAGES.get(stage).copied() {
            let s = if flags.digest {
                self.digest_stage(stage, stage_dir, &flags)
            } else {
                self.run_stage(host, stage, stage_dir, &flags)
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
        if flags.all_stages && total.failed == 0 {
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
            println!("[MISSING] {}", full_path.display());
            return RunSummary::default();
        }
        let mut tests = collect::collect_tests(&full_path);
        if let Some(ref json) = flags.failed_json {
            let before = tests.len();
            tests = collect::filter_by_failed_json(tests, json);
            if !flags.quick {
                println!(
                    "  RERUN failed-only: {} of {} from {}",
                    tests.len(),
                    before,
                    json
                );
            }
        }
        digest::run_stage_digest(&self.harness, stage, stage_dir, &tests, flags).summary
    }

    fn run_stage(
        &self,
        host: &mut dyn Test262Host,
        stage: usize,
        stage_dir: &str,
        flags: &RunnerFlags,
    ) -> RunSummary {
        let full_path = self.test262_dir.join(stage_dir);
        if !full_path.exists() {
            println!("[MISSING] {}", full_path.display());
            return RunSummary::default();
        }
        let tests = collect::collect_tests(&full_path);
        let count = tests.len();
        if !flags.quick {
            println!("\n=== Stage {}: {} ({} tests) ===", stage, stage_dir, count);
        }
        let mut summary = RunSummary::default();
        for (i, path) in tests.iter().enumerate() {
            match run_single_test(host, &self.harness, path) {
                TestOutcome::Pass => {
                    summary.passed += 1;
                    if !flags.quick && summary.passed % 100 == 0 {
                        println!("  ... {} passed", summary.passed);
                    }
                }
                TestOutcome::Skip { reason } => {
                    summary.skipped += 1;
                    if !flags.quick && summary.skipped <= 3 {
                        println!("  SKIP {} ({})", path.display(), reason);
                    }
                }
                TestOutcome::Fail { reason } => {
                    summary.failed += 1;
                    summary.first_failure = Some((path.display().to_string(), reason.clone()));
                    print_first_failure(stage, i, path, &reason);
                    break;
                }
            }
        }
        print_stage_footer(stage, count, &summary);
        summary
    }
}

fn print_first_failure(stage: usize, i: usize, path: &std::path::Path, reason: &str) {
    let src_diag = std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .take(20)
        .collect::<Vec<_>>()
        .join("\n");
    println!(
        "\n============================================================\n\
         FIRST FAILURE\n\
         Stage {} | #{}\n\
         {}\n\
         ------------------------------------------------------------\n\
         Reason: {}\n\
         ------------------------------------------------------------\n\
         Test source (first 20 lines):\n{}\n\
         ============================================================",
        stage,
        i,
        path.display(),
        reason,
        src_diag
    );
}

fn print_stage_footer(stage: usize, count: usize, summary: &RunSummary) {
    if summary.failed == 0 {
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
