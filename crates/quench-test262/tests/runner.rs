use std::sync::{Arc, Mutex};

use quench_test262::{HarnessCache, Test262Host, Test262Runner, TestMetadata, TestOutcome};

struct Probe;

impl Test262Host for Probe {
    fn run_script(&mut self, source: &str) -> Result<(), String> {
        if source == "pass" {
            Ok(())
        } else {
            Err("probe failure".into())
        }
    }

    fn run_module_script(&mut self, _source: &str) -> Result<(), String> {
        Ok(())
    }

    fn run_harnessed_script(
        &mut self,
        _harness: &[&str],
        source: &str,
        _strict: bool,
    ) -> Result<(), String> {
        self.run_script(source)
    }

    fn run_harnessed_module(&mut self, _harness: &[&str], source: &str) -> Result<(), String> {
        self.run_module_script(source)
    }
}

struct NegativeProbe;

impl Test262Host for NegativeProbe {
    fn run_script(&mut self, _source: &str) -> Result<(), String> {
        Err("SyntaxError: invalid token".into())
    }

    fn run_module_script(&mut self, _source: &str) -> Result<(), String> {
        Err("TypeError: wrong error".into())
    }

    fn run_harnessed_script(
        &mut self,
        _harness: &[&str],
        source: &str,
        _strict: bool,
    ) -> Result<(), String> {
        self.run_script(source)
    }

    fn run_harnessed_module(&mut self, _harness: &[&str], source: &str) -> Result<(), String> {
        self.run_module_script(source)
    }
}

struct HarnessProbe;

impl Test262Host for HarnessProbe {
    fn run_script(&mut self, source: &str) -> Result<(), String> {
        if source.contains("harness") && source.contains("\"use strict\";") {
            Ok(())
        } else {
            Err("harness composition missing".into())
        }
    }

    fn run_module_script(&mut self, _source: &str) -> Result<(), String> {
        Ok(())
    }

    fn run_harnessed_script(
        &mut self,
        harness: &[&str],
        _source: &str,
        strict: bool,
    ) -> Result<(), String> {
        if harness.iter().any(|source| source.contains("harness")) && strict {
            Ok(())
        } else {
            Err("harness composition missing".into())
        }
    }

    fn run_harnessed_module(&mut self, _harness: &[&str], source: &str) -> Result<(), String> {
        self.run_module_script(source)
    }
}

struct CaptureHost {
    seen: Arc<Mutex<Vec<String>>>,
}

impl Test262Host for CaptureHost {
    fn run_script(&mut self, source: &str) -> Result<(), String> {
        self.seen.lock().unwrap().push(source.to_string());
        Ok(())
    }

    fn run_module_script(&mut self, source: &str) -> Result<(), String> {
        self.seen.lock().unwrap().push(source.to_string());
        Ok(())
    }

    fn run_harnessed_script(
        &mut self,
        harness: &[&str],
        source: &str,
        strict: bool,
    ) -> Result<(), String> {
        let observed = harness
            .iter()
            .copied()
            .chain(strict.then_some("\"use strict\";"))
            .chain(std::iter::once(source))
            .collect::<Vec<_>>()
            .join("\n");
        self.seen.lock().unwrap().push(observed);
        Ok(())
    }

    fn run_harnessed_module(&mut self, harness: &[&str], source: &str) -> Result<(), String> {
        self.run_harnessed_script(harness, source, true)
    }
}

#[test]
fn runner_maps_engine_result_to_test_outcome() {
    let mut runner = Test262Runner::new(Probe);
    assert_eq!(runner.run_script("pass"), TestOutcome::Pass);
    assert_eq!(
        runner.run_script("fail"),
        TestOutcome::Fail {
            reason: "probe failure".into()
        }
    );
}

#[test]
fn metadata_parses_module_and_negative_expectation() {
    let source = r#"/*---
flags: [module]
negative:
  phase: parse
  type: SyntaxError
---*/
export default 1;
"#;
    let metadata = TestMetadata::parse(source).unwrap();
    assert!(metadata.is_module);
    assert_eq!(metadata.negative_phase.as_deref(), Some("parse"));
    assert_eq!(metadata.negative_type.as_deref(), Some("SyntaxError"));
}

#[test]
fn metadata_parses_async_strict_and_includes_flags() {
    let source = r#"/*---
flags: [async, onlyStrict]
includes: [assert.js, sta.js]
---*/
"#;
    let metadata = TestMetadata::parse(source).unwrap();
    assert!(metadata.is_async);
    assert!(metadata.only_strict);
    assert_eq!(metadata.includes, vec!["assert.js", "sta.js"]);
}

#[test]
fn runner_uses_frontmatter_to_select_module_dispatch() {
    let source = "/*---\nflags: [module]\n---*/\nexport default 1;";
    let mut runner = Test262Runner::new(Probe);
    assert_eq!(runner.run_test(source).unwrap(), TestOutcome::Pass);
}

#[test]
fn runner_accepts_expected_negative_error() {
    let source = "/*---\nnegative:\n  phase: parse\n  type: SyntaxError\n---*/\ninvalid";
    let mut runner = Test262Runner::new(NegativeProbe);
    assert_eq!(runner.run_test(source).unwrap(), TestOutcome::Pass);
}

#[test]
fn runner_rejects_missing_or_wrong_negative_error() {
    let missing = "/*---\nnegative:\n  phase: parse\n  type: SyntaxError\n---*/\nvalid";
    let mut passing_runner = Test262Runner::new(Probe);
    assert!(matches!(
        passing_runner.run_test(missing).unwrap(),
        TestOutcome::Fail { .. }
    ));

    let wrong = "/*---\nflags: [module]\nnegative:\n  phase: parse\n  type: SyntaxError\n---*/\nexport default 1;";
    let mut wrong_runner = Test262Runner::new(NegativeProbe);
    assert!(matches!(
        wrong_runner.run_test(wrong).unwrap(),
        TestOutcome::Fail { .. }
    ));
}

#[test]
fn runner_composes_includes_and_only_strict_before_dispatch() {
    let source = "/*---\nflags: [onlyStrict]\nincludes: [assert.js]\n---*/\npass";
    let mut runner = Test262Runner::new(HarnessProbe);
    let outcome = runner
        .run_test_with_harness(source, |name| Ok(format!("// {name} harness")))
        .unwrap();
    assert_eq!(outcome, TestOutcome::Pass);
}

#[test]
fn runner_composes_async_harness_before_declared_includes() {
    let source =
        "/*---\nflags: [async, onlyStrict]\nincludes: [asyncHelpers.js, propertyHelper.js]\n---*/\npass";
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut runner = Test262Runner::new(CaptureHost { seen: seen.clone() });

    let outcome = runner
        .run_test_with_harness(source, |name| Ok(format!("// {name} harness")))
        .unwrap();

    assert_eq!(outcome, TestOutcome::Pass);
    let composed = seen.lock().unwrap().pop().unwrap();
    assert!(
        composed.find("// assert.js harness").unwrap()
            < composed.find("// sta.js harness").unwrap()
    );
    assert!(
        composed.find("// sta.js harness").unwrap()
            < composed.find("// doneprintHandle.js harness").unwrap()
    );
    assert!(
        composed.find("// doneprintHandle.js harness").unwrap()
            < composed.find("// asyncHelpers.js harness").unwrap()
    );
    assert!(
        composed.find("// asyncHelpers.js harness").unwrap()
            < composed.find("// propertyHelper.js harness").unwrap()
    );
    assert!(
        composed.find("// propertyHelper.js harness").unwrap()
            < composed.find("\"use strict\";").unwrap()
    );
    assert!(composed.find("\"use strict\";").unwrap() < composed.rfind("pass").unwrap());
}

#[test]
fn runner_keeps_raw_async_tests_unmodified() {
    let source = "/*---\nflags: [raw, async]\nincludes: [asyncHelpers.js]\n---*/\npass";
    let seen = Arc::new(Mutex::new(Vec::new()));
    let load_count = Arc::new(Mutex::new(0usize));
    let mut runner = Test262Runner::new(CaptureHost { seen: seen.clone() });

    let outcome = runner
        .run_test_with_harness(source, {
            let load_count = load_count.clone();
            move |_| {
                *load_count.lock().unwrap() += 1;
                Ok(String::new())
            }
        })
        .unwrap();

    assert_eq!(outcome, TestOutcome::Pass);
    assert_eq!(*load_count.lock().unwrap(), 0);
    assert_eq!(seen.lock().unwrap().pop().unwrap(), source);
}

#[test]
fn runner_loads_test_source_from_a_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("case.js");
    std::fs::write(&path, "pass").unwrap();
    let mut runner = Test262Runner::new(Probe);
    assert_eq!(runner.run_file(&path).unwrap(), TestOutcome::Pass);
}

#[test]
fn runner_loads_file_and_composes_its_harness() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("case.js");
    std::fs::write(
        &path,
        "/*---\nflags: [onlyStrict]\nincludes: [assert.js]\n---*/\npass",
    )
    .unwrap();
    let mut runner = Test262Runner::new(HarnessProbe);
    let outcome = runner
        .run_file_with_harness(&path, |name| Ok(format!("// {name} harness")))
        .unwrap();
    assert_eq!(outcome, TestOutcome::Pass);
}

#[test]
fn cached_harness_preserves_declared_composition_order() {
    let dir = tempfile::tempdir().unwrap();
    let harness_root = dir.path().join("harness");
    std::fs::create_dir(&harness_root).unwrap();
    std::fs::write(harness_root.join("assert.js"), "// assert").unwrap();
    std::fs::write(harness_root.join("sta.js"), "// sta").unwrap();
    std::fs::write(harness_root.join("helper.js"), "// helper").unwrap();
    let source = "/*---\nflags: [onlyStrict]\nincludes: [helper.js]\n---*/\npass";
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut runner = Test262Runner::new(CaptureHost { seen: seen.clone() });
    let mut cache = HarnessCache::new(harness_root);

    assert_eq!(
        runner.run_test_with_cache(source, &mut cache).unwrap(),
        TestOutcome::Pass
    );

    let composed = seen.lock().unwrap().pop().unwrap();
    assert!(composed.starts_with("// assert\n// sta\n// helper\n\"use strict\";\n/*---"));
}

#[test]
fn runner_reports_batch_file_outcomes() {
    let dir = tempfile::tempdir().unwrap();
    let passing = dir.path().join("pass.js");
    let failing = dir.path().join("fail.js");
    std::fs::write(&passing, "pass").unwrap();
    std::fs::write(&failing, "fail").unwrap();
    let mut runner = Test262Runner::new(Probe);
    let report = runner.run_files([passing, failing]).unwrap();
    assert_eq!(report.total, 2);
    assert_eq!(report.passed, 1);
    assert_eq!(report.failed, 1);
}

#[test]
fn runner_reports_batch_file_read_failures_without_aborting() {
    let dir = tempfile::tempdir().unwrap();
    let passing = dir.path().join("pass.js");
    let missing = dir.path().join("missing.js");
    std::fs::write(&passing, "pass").unwrap();
    let mut runner = Test262Runner::new(Probe);

    let report = runner.run_files([passing, missing.clone()]).unwrap();

    assert_eq!(report.total, 2);
    assert_eq!(report.passed, 1);
    assert_eq!(report.failed, 1);
    assert_eq!(report.failures[0].0, missing);
    assert!(report.failures[0].1.contains("test262 read failed"));
}

#[test]
fn runner_reports_harness_aware_batch_outcomes() {
    let dir = tempfile::tempdir().unwrap();
    let passing = dir.path().join("pass.js");
    let failing = dir.path().join("fail.js");
    std::fs::write(
        &passing,
        "/*---\nflags: [onlyStrict]\nincludes: [assert.js]\n---*/\npass",
    )
    .unwrap();
    std::fs::write(&failing, "fail").unwrap();
    let mut runner = Test262Runner::new(HarnessProbe);

    let report = runner
        .run_files_with_harness([passing, failing], |name| Ok(format!("// {name} harness")))
        .unwrap();

    assert_eq!(report.total, 2);
    assert_eq!(report.passed, 1);
    assert_eq!(report.failed, 1);
}

#[test]
fn discovers_skips_fixture_files() {
    // *_FIXTURE.js files are harness fixtures, not standalone tests; they must
    // not be collected as runnable cases (the runner excludes
    // them too).
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.js"), "pass").unwrap();
    std::fs::write(dir.path().join("b_FIXTURE.js"), "not a test").unwrap();
    std::fs::write(dir.path().join("c_FIXTURE.js"), "not a test").unwrap();
    let files = quench_test262::discover_js_files(dir.path()).unwrap();
    assert_eq!(files, vec![dir.path().join("a.js")]);
}

#[test]
fn discovers_js_files_recursively_in_sorted_order() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("nested")).unwrap();
    std::fs::write(dir.path().join("z.js"), "pass").unwrap();
    std::fs::write(dir.path().join("nested/a.js"), "pass").unwrap();
    std::fs::write(dir.path().join("ignore.txt"), "pass").unwrap();
    let files = quench_test262::discover_js_files(dir.path()).unwrap();
    assert_eq!(
        files,
        vec![dir.path().join("nested/a.js"), dir.path().join("z.js")]
    );
}

#[test]
fn standalone_runner_exposes_only_host_facing_execution() {
    let mut runner = Test262Runner::new(Probe);
    assert_eq!(runner.run_script("pass"), TestOutcome::Pass);
}
