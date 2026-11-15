//! The suspending functions the lane once left to the frame fallback (`Vm::run_job`) run as
//! windows: locals promoted to registers beside closures, a rest parameter, `arguments` or
//! parameters left in the frame's locals, an `arguments` object a closure captures, and an Annex B
//! store to a `var` binding. Each lays out an environment window and runs with no frame made.
use super::window_steps::tests::{agree, layouts, run};

/// Asserts every suspending function of `source` lays out a window, that `source` prints
/// `expected` with no frame made, and that the execution modes agree.
fn runs_in_a_window(source: &str, expected: &str) {
    assert!(layouts(source).iter().all(|(registers, environment)| *registers || *environment), "a function laid out no window");
    assert_eq!(agree(source), [expected]);
    let (lines, made) = run(source);
    assert_eq!(lines, [expected]);
    assert_eq!(made, 0, "a resume made a frame");
}

#[test]
fn promoted_registers_beside_a_closure_ride_a_window() {
    runs_in_a_window(
        r#"function* counting(n) {
               let total = 0;
               const add = (k) => total += k;
               for (let i = 0; i < n; i++) { add(i); yield total; }
           }
           print([...counting(5)].join(","));"#,
        "0,1,3,6,10",
    );
    runs_in_a_window(
        r#"async function summing(n) {
               let sum = 0;
               const add = (k) => sum += k;
               for (let i = 0; i < n; i++) add(await i);
               return sum;
           }
           summing(6).then((sum) => print(String(sum)));"#,
        "15",
    );
}

#[test]
fn promoted_registers_beside_a_rest_parameter_ride_a_window() {
    runs_in_a_window(
        r#"function* running(...values) {
               let total = 0;
               for (let i = 0; i < values.length; i++) { total += values[i]; yield total; }
           }
           print([...running(1, 2, 3, 4)].join(","));"#,
        "1,3,6,10",
    );
}

#[test]
fn promoted_registers_beside_arguments_ride_a_window() {
    runs_in_a_window(
        r#"function* running() {
               let total = 0;
               for (let i = 0; i < arguments.length; i++) { total += arguments[i]; yield total; }
           }
           print([...running(2, 3, 4)].join(","));"#,
        "2,5,9",
    );
}

#[test]
fn parameters_in_the_frames_locals_beside_promoted_registers_ride_a_window() {
    runs_in_a_window(
        r#"function* offset(count, base, step) {
               let value = base;
               for (let i = 0; i < count; i++) { yield value; value += step; }
           }
           print([...offset(4, 10, 5)].join(","));"#,
        "10,15,20,25",
    );
}

#[test]
fn an_arguments_object_a_closure_captures_is_an_environment_slot_of_the_window() {
    // The object made on entry is a window local until the closure makes the environment, and its
    // slot there after, which reads and the mapped parameters' writes find alike.
    let source = r#"function* captured(a) {
               const length = () => arguments.length;
               yield length;
               a = 7;
               yield arguments[0];
               arguments[0] = 8;
               yield a;
           }
           const out = [];
           for (const x of captured(1, 2, 3)) out.push(typeof x === "function" ? x() : x);
           print(out.join(","));"#;
    assert_eq!(layouts(source), [(false, true)]);
    runs_in_a_window(source, "3,7,8");
}

#[test]
fn an_annex_b_block_function_stores_to_its_var_binding_in_the_window() {
    runs_in_a_window(
        r#"function* hoisting() {
               { function inner() { return 1; } }
               yield typeof inner;
               inner = 3;
               yield inner;
           }
           print([...hoisting()].join(","));"#,
        "function,3",
    );
}
