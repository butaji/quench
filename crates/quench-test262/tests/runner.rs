use quench_runtime::Test262Host;
use quench_test262::{Test262Runner, TestMetadata, TestOutcome};

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
}

struct NegativeProbe;

impl Test262Host for NegativeProbe {
    fn run_script(&mut self, _source: &str) -> Result<(), String> {
        Err("SyntaxError: invalid token".into())
    }

    fn run_module_script(&mut self, _source: &str) -> Result<(), String> {
        Err("TypeError: wrong error".into())
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
fn runner_loads_test_source_from_a_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("case.js");
    std::fs::write(&path, "pass").unwrap();
    let mut runner = Test262Runner::new(Probe);
    assert_eq!(runner.run_file(&path).unwrap(), TestOutcome::Pass);
}
