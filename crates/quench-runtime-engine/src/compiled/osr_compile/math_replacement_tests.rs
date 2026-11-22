use super::tests::osr_run;

/// The result of `run(200000)` where `Math.floor(i & 1023)` is the identity for the first 150000 iterations and a constant 1000 after:
/// the replaced function's value must be the one the later iterations add.
fn expected_after_replacement() -> String {
    let before: i64 = (0..150_000i64).map(|i| i & 1023).sum();
    (before + 1000 * 50_000).to_string()
}

/// `Math` (the global) replaced inside the hot loop: the loop's `Math.floor` reads the new object.
#[test]
fn a_global_math_replaced_in_a_hot_loop_is_the_one_the_next_calls_run() {
    let (output, _) = quench_runtime::jit_mode::with_mode(quench_runtime::JitMode::BaseOnly, || osr_run(
        "var fake = { floor: function (x) { return 1000; } };
         function run(n) {
           var s = 0;
           for (var i = 0; i < n; i++) {
             if (i === 150000) Math = fake;
             s = (s + Math.floor(i & 1023)) | 0;
           }
           return s;
         }
         print(run(200000));", "run"));
    assert_eq!(output, vec![expected_after_replacement()]);
}

/// `Math.floor` (the member) replaced inside the hot loop: the loop's call runs the replacement, though `Math` keeps its shape.
#[test]
fn a_math_member_replaced_in_a_hot_loop_is_the_one_the_next_calls_run() {
    let (output, _) = quench_runtime::jit_mode::with_mode(quench_runtime::JitMode::BaseOnly, || osr_run(
        "function run(n) {
           var s = 0;
           for (var i = 0; i < n; i++) {
             if (i === 150000) Math.floor = function (x) { return 1000; };
             s = (s + Math.floor(i & 1023)) | 0;
           }
           return s;
         }
         print(run(200000));", "run"));
    assert_eq!(output, vec![expected_after_replacement()]);
}

/// `Math` replaced by a function called in the hot loop: the call's effect on the global is seen by the loop's next `Math.floor`.
#[test]
fn a_global_math_replaced_by_a_function_called_in_a_hot_loop_is_the_one_the_next_calls_run() {
    let (output, _) = quench_runtime::jit_mode::with_mode(quench_runtime::JitMode::BaseOnly, || osr_run(
        "var fake = { floor: function (x) { return 1000; } };
         function swap() { Math = fake; }
         function run(n) {
           var s = 0;
           for (var i = 0; i < n; i++) {
             if (i === 150000) swap();
             s = (s + Math.floor(i & 1023)) | 0;
           }
           return s;
         }
         print(run(200000));", "run"));
    assert_eq!(output, vec![expected_after_replacement()]);
}

/// `Math.floor` replaced by a function called in the hot loop: the member's change reaches the loop's next call.
#[test]
fn a_math_member_replaced_by_a_function_called_in_a_hot_loop_is_the_one_the_next_calls_run() {
    let (output, _) = quench_runtime::jit_mode::with_mode(quench_runtime::JitMode::BaseOnly, || osr_run(
        "function swap() { Math.floor = function (x) { return 1000; }; }
         function run(n) {
           var s = 0;
           for (var i = 0; i < n; i++) {
             if (i === 150000) swap();
             s = (s + Math.floor(i & 1023)) | 0;
           }
           return s;
         }
         print(run(200000));", "run"));
    assert_eq!(output, vec![expected_after_replacement()]);
}
