//! A direct eval a lane activation runs (`lane_eval`): the activation's record owns the eval's
//! variable environment, the eval promotes the window's environment into it, and the activation
//! continues in the lane reading what the eval declared. Compiled runs agree with the
//! interpreter's.
use super::window_steps::tests::{agree, run};

#[test]
fn a_hot_ordinary_function_runs_its_direct_eval_in_the_lane() {
    let source = r#"
        function step(n) {
            var x = n;
            let y = 2;
            eval("var z = x + y; x = x + 1");
            return z + x + typeof eval("y");
        }
        let sum = "";
        for (let i = 0; i < 3000; i++) sum = step(i);
        print(sum);
        "#;
    assert_eq!(agree(source), ["6001number"]);
    let (lines, made) = run(source);
    assert_eq!(lines, ["6001number"]);
    assert_eq!(made, 0, "an activation became a frame");
}

#[test]
fn a_direct_eval_declares_its_vars_in_the_activations_environment_for_later_reads() {
    let lines = agree(
        r#"
        function declare(n) {
            eval("var late = " + n);
            const read = () => late + 1;
            return [late, read(), typeof eval("late")].join(",");
        }
        let last = "";
        for (let i = 0; i < 2000; i++) last = declare(i);
        print(last);
        "#,
    );
    assert_eq!(lines, ["1999,2000,number"]);
}

#[test]
fn a_generator_using_eval_across_yields_makes_no_frame() {
    let (lines, made) = run(
        r#"
        function* gen() {
            var a = 1;
            yield eval("var b = a + 1; b");
            a = 10;
            yield eval("a + b");
            yield b;
        }
        print([...gen()].join(","));
        "#,
    );
    assert_eq!(lines, ["2,12,2"]);
    assert_eq!(made, 0, "a resume made a frame");
}

#[test]
fn an_eval_raising_in_the_lane_is_caught_by_the_activations_handler() {
    let lines = agree(
        r#"
        function guarded(code) {
            try { return eval(code); } catch (e) { return e.name; }
        }
        let last = "";
        for (let i = 0; i < 1000; i++) last = guarded(i % 2 ? "null.x" : "1 + 1");
        print(last + guarded("1 +"));
        "#,
    );
    assert_eq!(lines, ["TypeErrorSyntaxError"]);
}

#[test]
fn a_generator_step_run_by_eval_code_is_a_nested_job_run_in_the_lane() {
    // The eval's code runs above the generator's activation, whose lane stack parks
    // (`Vm::nested_lane_run`); the inner generator's `next` is a job run of its own over a fresh
    // one, in the lane: no frame is made for either generator.
    let (lines, made) = run(
        r#"
        function* inner() { yield 1; yield 2; yield 3; }
        function* outer() {
            const it = inner();
            yield eval("it.next().value + it.next().value");
            yield eval("it.next().value");
        }
        print([...outer()].join(","));
        "#,
    );
    assert_eq!(lines, ["3,3"]);
    assert_eq!(made, 0, "a resume made a frame");
}

#[test]
fn a_collection_inside_a_nested_eval_run_keeps_the_values_of_the_parked_activations() {
    // The eval's allocations make collections due while the activation that called it is parked
    // (`Vm::nested_lane_run`): its window's objects and strings, and the receiver its context
    // holds, survive them (`LaneStack::extend_roots`).
    let lines = agree(
        r#"
        function churn(n) {
            const kept = { tag: "kept", list: [1, 2, 3] };
            const name = "k" + n;
            eval("var made = 0; for (let i = 0; i < 20000; i++) made += [i, { i }, 'x' + i].length;");
            return kept.tag + kept.list.join("") + name + made;
        }
        let last = "";
        for (let i = 0; i < 30; i++) last = churn(i);
        print(last);
        "#,
    );
    assert_eq!(lines, ["kept123k2960000"]);
}
