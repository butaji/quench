use quench_runtime::Test262Host;
use quench_test262::{Test262Runner, TestOutcome};

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
