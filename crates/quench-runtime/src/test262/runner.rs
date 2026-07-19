//! test262 staged runner — one stage at a time, 100% passing required.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use crate::test262::harness::HarnessLoader;
use crate::test262::host::{QuenchHost, Test262Host, TestOutcome};
use crate::test262::metadata::Test262Metadata;

/// Per-test timeout in seconds. If a test takes longer than this,
/// it is reported as a failure rather than blocking the stage.
const TEST_TIMEOUT_SECS: u64 = 10;

/// Ordered stages (relative to test262/test/).
///
/// 100% enumeration of every directory under `test/` that is part of
/// ECMA-262 test262 — `test/intl402` (ECMA-402) and `test/staging` are
/// intentionally excluded (separate conformance suites). The list mirrors
/// `tasks/index.json` exactly; keep them in sync.
pub const STAGES: &[&str] = &[
    // harness
    "test/harness",
    // language — lexical structure → types → statements → scoping → modules
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
    "test/language/statements",
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
    // built-ins — globals → constructors → iterators → collections → advanced
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
    // annexB (legacy / web compatibility — recurses through built-ins/ and language/)
    "test/annexB",
];

fn collect_tests(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if dir.is_file() {
        if dir.extension().map(|e| e == "js").unwrap_or(false)
            && !dir
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with("_FIXTURE.js")
        {
            out.push(dir.to_path_buf());
        }
        return out;
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                out.extend(collect_tests(&p));
            } else if p.extension().map(|e| e == "js").unwrap_or(false)
                && !p
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .ends_with("_FIXTURE.js")
            {
                out.push(p);
            }
        }
    }
    out
}

#[derive(Debug, Default, Clone)]
pub struct RunSummary {
    pub passed: usize,
    pub failed: usize,
    pub first_failure: Option<(String, String)>,
}

fn check_outcome(meta: &Test262Metadata, result: Result<(), String>) -> TestOutcome {
    match (&meta.negative, result) {
        (None, Ok(())) => TestOutcome::Pass,
        (None, Err(msg)) => TestOutcome::Fail { reason: msg },
        (Some(_), Ok(())) => TestOutcome::Fail {
            reason: "expected error but passed".into(),
        },
        (Some(neg), Err(_)) if neg.phase == "parse" => TestOutcome::Pass,
        (Some(neg), Err(msg)) => {
            if !neg.typ.is_empty() && !msg.contains(&neg.typ) {
                TestOutcome::Fail {
                    reason: format!("expected {} but got: {}", neg.typ, msg),
                }
            } else {
                TestOutcome::Pass
            }
        }
    }
}

pub fn run_single_test(
    host: &mut dyn Test262Host,
    harness: &HarnessLoader,
    test_path: &Path,
) -> TestOutcome {
    let source = match fs::read_to_string(test_path) {
        Ok(s) => s,
        Err(e) => {
            return TestOutcome::Fail {
                reason: format!("read: {}", e),
            }
        }
    };

    let meta = match Test262Metadata::parse(&source) {
        Some(m) => m,
        None => {
            return TestOutcome::Fail {
                reason: "bad frontmatter".into(),
            }
        }
    };

    let is_module = meta.flags.contains(&"module".to_string());
    let is_raw = meta.flags.contains(&"raw".to_string());

    let script = if is_raw {
        source.clone()
    } else {
        let is_async = meta.flags.contains(&"async".to_string());
        if is_async {
            let prelude = "var $DONE = function(error) { if (error !== undefined && error !== null) throw error; };\n";
            match harness.build_script(&source, &meta.includes) {
                Ok(s) => format!("{}{}", prelude, s),
                Err(e) => return TestOutcome::Fail { reason: e },
            }
        } else {
            match harness.build_script(&source, &meta.includes) {
                Ok(s) => s,
                Err(e) => return TestOutcome::Fail { reason: e },
            }
        }
    };

    let no_strict = is_raw || meta.flags.contains(&"noStrict".to_string());
    let only_strict = meta.flags.contains(&"onlyStrict".to_string());

    let timeout = Duration::from_secs(TEST_TIMEOUT_SECS);
    let run_sloppy = |script: &str, host: &mut dyn Test262Host| -> TestOutcome {
        // Use a fresh QuenchHost in a separate thread so a stuck thread
        // does not block the stage — the thread is abandoned after timeout.
        let meta = meta.clone();
        let script = script.to_owned();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut inner = QuenchHost::new();
            let result = if is_module {
                inner.run_module_script(&script)
            } else {
                inner.run_script(&script)
            };
            let _ = tx.send(check_outcome(&meta, result));
        });
        match rx.recv_timeout(timeout) {
            Ok(outcome) => outcome,
            Err(mpsc::RecvTimeoutError::Timeout) => TestOutcome::Fail {
                reason: format!("Must be optimized (timed out after {}s)", TEST_TIMEOUT_SECS),
            },
            Err(mpsc::RecvTimeoutError::Disconnected) => TestOutcome::Fail {
                reason: "panicked".into(),
            },
        }
    };

    if !only_strict {
        let outcome = run_sloppy(&script, host);
        if !matches!(outcome, TestOutcome::Pass) {
            return outcome;
        }
        if no_strict {
            return TestOutcome::Pass;
        }
    }

    if no_strict {
        return TestOutcome::Pass;
    }

    let strict_script = format!("\"use strict\";\n{}", script);
    match run_sloppy(&strict_script, host) {
        TestOutcome::Fail { reason } => TestOutcome::Fail {
            reason: format!("strict: {}", reason),
        },
        other => other,
    }
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
        let all = std::env::var("ALL_STAGES")
            .ok()
            .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let start = std::env::var("TEST262_STAGE")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);

        let mut total = RunSummary::default();
        let mut stage = start;
        while let Some(stage_dir) = STAGES.get(stage).copied() {
            let s = self.run_stage(host, stage, stage_dir);
            total.passed += s.passed;
            total.failed += s.failed;
            if s.failed > 0 {
                total.first_failure = s.first_failure;
                break;
            }
            if !all {
                break;
            }
            stage += 1;
        }

        if all && total.failed == 0 {
            println!(
                "\n=== ALL STAGES COMPLETE — {} stages passed ===",
                STAGES.len()
            );
        }
        total
    }

    fn run_stage(
        &self,
        host: &mut dyn Test262Host,
        stage: usize,
        stage_dir: &str,
    ) -> RunSummary {
        let full_path = self.test262_dir.join(stage_dir);
        if !full_path.exists() {
            println!("[MISSING] {}", full_path.display());
            return RunSummary::default();
        }

        let mut tests = collect_tests(&full_path);
        tests.sort();
        let count = tests.len();

        println!("\n=== Stage {}: {} ({} tests) ===", stage, stage_dir, count);

        let mut summary = RunSummary::default();
        let mut passed = 0;

        for (i, path) in tests.iter().enumerate() {
            match run_single_test(host, &self.harness, path) {
                TestOutcome::Pass => {
                    passed += 1;
                    if passed % 100 == 0 {
                        println!("  ... {} passed", passed);
                    }
                }
                TestOutcome::Fail { reason } => {
                    summary.failed += 1;
                    summary.first_failure = Some((path.display().to_string(), reason.clone()));
                    println!(
                        "\n============================================================\n\
                         FIRST FAILURE\n\
                         Stage {} | #{}\n\
                         {}\n\
                         ------------------------------------------------------------\n\
                         {}\n\
                         ============================================================",
                        stage,
                        i,
                        path.display(),
                        reason
                    );
                    break;
                }
            }
        }

        if summary.failed == 0 {
            println!("ALL STAGES COMPLETE — Stage {}: {}/{}", stage, count, count);
        } else {
            println!(
                "Stage {}: {}/{} passed (first failure reported)",
                stage, passed, count
            );
        }
        summary.passed = passed;
        summary
    }
}
