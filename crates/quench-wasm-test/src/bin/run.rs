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
    const DEFAULT_FAILURE_LIMIT: usize = 2000;
    let failure_limit = env::var("QUENCH_WASM_FAILURE_LIMIT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_FAILURE_LIMIT);
    for failure in report.failures.iter().take(failure_limit) {
        println!("{}", failure.format_line());
    }
    if report.failures.len() > failure_limit {
        println!(
            "... and {} more failures",
            report.failures.len() - failure_limit
        );
    }
    if report.failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
