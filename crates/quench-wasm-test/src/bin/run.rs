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
    const PRINT_LIMIT: usize = 2000;
    for failure in report.failures.iter().take(PRINT_LIMIT) {
        println!("{}", failure.format_line());
    }
    if report.failures.len() > PRINT_LIMIT {
        println!(
            "... and {} more failures",
            report.failures.len() - PRINT_LIMIT
        );
    }
    if report.failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
