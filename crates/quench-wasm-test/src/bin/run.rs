use std::{env, process::ExitCode};

fn main() -> ExitCode {
    let root = env::args_os()
        .nth(1)
        .map_or_else(quench_wasm_test::testsuite_root, Into::into);
    let report = quench_wasm_test::TestSuite::new(root).run_all();
    println!(
        "wasm tests: {} total, {} passed, {} failed",
        report.total, report.passed, report.failed
    );
    if report.failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
