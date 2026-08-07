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
fn runner_uses_frontmatter_to_select_module_dispatch() {
    let source = "/*---\nflags: [module]\n---*/\nexport default 1;";
    let mut runner = Test262Runner::new(Probe);
    assert_eq!(runner.run_test(source).unwrap(), TestOutcome::Pass);
}
