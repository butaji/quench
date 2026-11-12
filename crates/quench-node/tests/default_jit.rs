//! `quench-node` runs compiled code by default; `--no-jit` alone selects the interpreter.
use std::process::Command;

const HOT_LOOP: &str = "function f(n){let s=0;for(let i=0;i<n;i++)s=(s+i)|0;return s} for(let k=0;k<200;k++)f(100000);";

/// The compiles the run's JIT statistics report: function compiles and loop-region compiles.
fn compiles(arguments: &[&str]) -> u64 {
    let output = Command::new(env!("CARGO_BIN_EXE_quench-node"))
        .args(arguments)
        .args(["-e", HOT_LOOP])
        .env("QUENCH_TRACE", "js-jit")
        .env_remove("NODE_OPTIONS")
        .output()
        .expect("run quench-node");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .filter_map(|line| line.strip_prefix("QUENCH_JS_JIT_STATS "))
        .map(|json| count_after(json, "\"compile_successes\":"))
        .sum()
}

fn count_after(json: &str, key: &str) -> u64 {
    json.split(key).skip(1)
        .map(|rest| rest.chars().take_while(char::is_ascii_digit).collect::<String>().parse::<u64>().unwrap_or(0))
        .sum()
}

#[test]
fn a_hot_loop_compiles_by_default_and_not_under_no_jit() {
    assert!(compiles(&[]) > 0, "no flag: the hot loop must run compiled code");
    assert_eq!(compiles(&["--no-jit"]), 0, "--no-jit: nothing compiles");
}

/// Run `quench-node` with the flags and a trivial program; the exit status and stderr.
fn run_flags(arguments: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_quench-node"))
        .args(arguments)
        .args(["-e", "1"])
        .env_remove("NODE_OPTIONS")
        .output()
        .expect("run quench-node");
    (output.status.success(), String::from_utf8_lossy(&output.stderr).into_owned())
}

#[test]
fn the_jit_mode_flags_select_one_mode_and_the_old_flag_is_gone() {
    for flag in ["--no-jit", "--b-jit", "--u-jit"] {
        assert!(run_flags(&[flag]).0, "{flag} runs");
    }
    assert!(!run_flags(&["--no-jit", "--b-jit"]).0, "two modes are an error");
    assert!(!run_flags(&["--b-jit", "--u-jit"]).0, "two modes are an error");
    assert!(!run_flags(&["--jit"]).0, "--jit is removed");
    assert!(!run_flags(&["--q-jit"]).0, "--q-jit is reserved for the Cranelift quick mode and rejected");
    assert!(!run_flags(&["--f-jit"]).0, "--f-jit is an unknown flag");
    assert!(compiles(&["--b-jit"]) > 0, "--b-jit: the hot number loop runs from stencils");
    assert!(compiles(&["--u-jit"]) > 0, "--u-jit: the hot loop runs compiled code");
}

/// Runs the fixture under `flag`; the exit status and stdout.
fn run_fixture(flag: &str, fixture: &str) -> (bool, String) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(fixture);
    let output = Command::new(env!("CARGO_BIN_EXE_quench-node")).arg(flag).arg(path).env_remove("NODE_OPTIONS").output().expect("run quench-node");
    (output.status.success(), String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Loops whose registers hold ints, then doubles, then ints, left at every guard (widening
/// recompiles, element and tag exits) leave the lane exactly as the interpreter has it.
#[test]
fn b_jit_exits_leave_the_interpreters_state() {
    for fixture in ["base_jit_webs.js", "base_jit_exit_state.js"] {
        let expected = run_fixture("--no-jit", fixture);
        assert!(expected.0, "{fixture} under --no-jit");
        for _ in 0..3 {
            assert_eq!(run_fixture("--b-jit", fixture), expected, "{fixture} under --b-jit");
        }
    }
}

/// The function-level compile attempts (before each function's `osr` section) and the loop-region runs of a run's statistics.
fn function_attempts_and_region_runs(flag: &str) -> (u64, u64) {
    let output = Command::new(env!("CARGO_BIN_EXE_quench-node"))
        .args([flag, "-e", HOT_LOOP])
        .env("QUENCH_TRACE", "js-jit")
        .env_remove("NODE_OPTIONS")
        .output()
        .expect("run quench-node");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let (mut attempts, mut runs) = (0, 0);
    for json in String::from_utf8_lossy(&output.stderr).lines().filter_map(|line| line.strip_prefix("QUENCH_JS_JIT_STATS ")) {
        let (function, osr) = json.split_once("\"osr\"").unwrap_or((json, ""));
        attempts += count_after(function, "\"compile_attempts\":") + count_after(function, "\"general_dispatch_compile_attempts\":");
        runs += count_after(osr, "\"native_region_runs\":");
    }
    (attempts, runs)
}

#[test]
fn b_jit_compiles_no_function_and_runs_base_regions() {
    let (attempts, runs) = function_attempts_and_region_runs("--b-jit");
    assert_eq!(attempts, 0, "--b-jit: no Cranelift function compile is attempted");
    assert!(runs > 0, "--b-jit: the hot loop runs its stencil region");
}
