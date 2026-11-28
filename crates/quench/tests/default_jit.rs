//! `quench` runs compiled code by default; `--no-jit` alone selects the interpreter.
use std::process::Command;

const HOT_LOOP: &str = "function f(n){let s=0;for(let i=0;i<n;i++)s=(s+i)|0;return s} for(let k=0;k<200;k++)f(100000);";

/// The compiles the run's JIT statistics report: function compiles and loop-region compiles.
fn compiles(arguments: &[&str]) -> u64 {
    let output = Command::new(env!("CARGO_BIN_EXE_quench"))
        .args(arguments)
        .args(["-e", HOT_LOOP])
        .env("QUENCH_TRACE", "js-jit")
        .env_remove("NODE_OPTIONS")
        .output()
        .expect("run quench");
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

/// Run `quench` with the flags and a trivial program; the exit status and stderr.
fn run_flags(arguments: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_quench"))
        .args(arguments)
        .args(["-e", "1"])
        .env_remove("NODE_OPTIONS")
        .output()
        .expect("run quench");
    (output.status.success(), String::from_utf8_lossy(&output.stderr).into_owned())
}

#[test]
fn the_jit_mode_flags_select_one_mode_and_the_removed_flags_are_gone() {
    assert!(run_flags(&["--no-jit"]).0, "--no-jit runs");
    assert!(!run_flags(&["--jit"]).0, "--jit is removed");
    for flag in ["--b-jit", "--q-jit", "--u-jit"] {
        let (ok, stderr) = run_flags(&[flag]);
        assert!(!ok && stderr.contains(&format!("unknown JIT flag {flag}")), "{flag} is removed: {stderr}");
    }
    let (ok, stderr) = run_flags(&["--zz-jit"]);
    assert!(!ok && stderr.contains("unknown JIT flag --zz-jit"), "any other --*-jit flag is an unknown-flag error: {stderr}");
}

/// Runs the fixture under `flag`; the exit status and stdout.
fn run_fixture(flag: &str, fixture: &str) -> (bool, String) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(fixture);
    let output = Command::new(env!("CARGO_BIN_EXE_quench")).arg(flag).arg(path).env_remove("NODE_OPTIONS").output().expect("run quench");
    (output.status.success(), String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Loops whose registers hold ints, then doubles, then ints, left at every guard (widening
/// recompiles, element and tag exits) leave the lane exactly as the interpreter has it.
#[test]
fn compiled_exits_leave_the_interpreters_state() {
    for fixture in ["base_jit_webs.js", "base_jit_exit_state.js"] {
        let expected = run_fixture("--no-jit", fixture);
        assert!(expected.0, "{fixture} under --no-jit");
        for _ in 0..3 {
            assert_eq!(run_fixture("--", fixture), expected, "{fixture} under the default mode");
        }
    }
}

/// Runs the fixture under `flag` with the JIT statistics on; the loop counters named by `keys`,
/// summed over the run's `osr` totals (before each record's per-site list).
fn fixture_osr_counts(flag: &str, fixture: &str, keys: [&str; 3]) -> [u64; 3] {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(fixture);
    let output = Command::new(env!("CARGO_BIN_EXE_quench")).arg(flag).arg(path)
        .env("QUENCH_TRACE", "js-jit").env_remove("NODE_OPTIONS").output().expect("run quench");
    let mut counts = [0; 3];
    for json in String::from_utf8_lossy(&output.stderr).lines().filter_map(|line| line.strip_prefix("QUENCH_JS_JIT_STATS ")) {
        let osr = json.split_once("\"osr\"").map_or("", |(_, osr)| osr);
        let totals = osr.split_once("\"sites\"").map_or(osr, |(totals, _)| totals);
        for (count, key) in counts.iter_mut().zip(keys) { *count += count_after(totals, &format!("\"{key}\":")); }
    }
    counts
}

/// A hot loop entered by each tier, passed from one tier to the next while it runs, and left
/// at guards prints what the interpreter prints: output identity across every JIT mode, and
/// the transitions happened (a region ran, and a loop body published and ran after it).
#[test]
fn osr_transitions_between_tiers_keep_the_output() {
    let expected = run_fixture("--no-jit", "osr_transitions.js");
    assert!(expected.0 && expected.1.lines().count() == 6, "--no-jit: {}", expected.1);
    // `--` ends the options: the default mode, every tier on.
    assert_eq!(run_fixture("--", "osr_transitions.js"), expected, "osr_transitions.js under the default mode");
    let keys = ["native_region_runs", "tier_ups", "body_runs"];
    let [regions, published, ran] = fixture_osr_counts("--", "osr_transitions.js", keys);
    // The default mode runs the loop in its body: a region is entered only where a trial
    // beats the lane's loop rate (`LoopTier`), so the loop's native tier here is the body.
    assert!(regions + ran > 0, "the default mode: the hot loop runs a native tier");
    assert!(published > 0 && ran > 0, "the default mode: a loop body publishes ({published}) and runs ({ran})");
}
