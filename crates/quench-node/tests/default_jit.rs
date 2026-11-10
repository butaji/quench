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
