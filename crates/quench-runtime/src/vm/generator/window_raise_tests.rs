//! What a windowed activation does with an exception: a catch binds the thrown value in the
//! window (`handler_edges`), a parameter a closure captured rides the window beside a mapped
//! `arguments` object (`ParameterHome`), and one no handler covers ends the activation in its
//! owner (`raised_windows`): an async function's promise rejects, a generator completes and its
//! caller unwinds. Each equals the interpreter's.
use super::window_steps::tests::{agree, layouts};

#[test]
fn a_catch_in_a_generator_window_binds_the_thrown_value_there() {
    // The catch's binding is captured by a closure, so each entry gets a fresh environment.
    let lines = agree(
        r#"
        function* g(n) {
            const fns = [];
            for (let i = 0; i < n; i++) {
                try { if (i % 2) throw i; } catch (e) { fns.push(() => e); }
                yield i;
            }
            return fns;
        }
        let total = 0;
        for (let round = 0; round < 20; round++) {
            const it = g(10);
            let step = it.next();
            while (!step.done) step = it.next();
            total += step.value.reduce((sum, f) => sum + f(), 0);
        }
        print(String(total));
        "#,
    );
    assert_eq!(lines, ["500"]);
}

#[test]
fn a_catch_in_a_hot_ordinary_function_binds_the_thrown_value_in_its_window() {
    // The function's body runs natively once hot, over the environment window its activation has:
    // the catch's captured binding gets a fresh environment per entry, as a frame's does.
    let lines = agree(
        r#"
        function pick(n) {
            const fns = [];
            for (let i = 0; i < n; i++) {
                try { if (i % 2) throw i; } catch (e) { fns.push(() => e); }
            }
            return fns.reduce((sum, f) => sum + f(), 0);
        }
        let total = 0;
        for (let round = 0; round < 2000; round++) total += pick(10);
        print(String(total));
        "#,
    );
    assert_eq!(lines, ["50000"]);
}

#[test]
fn a_captured_parameter_rides_the_window_across_yields_beside_a_mapped_arguments_object() {
    let source = r#"
        function* g(a, b) {
            yield arguments[0];
            a = 5;
            yield arguments[0] + (() => a + b)();
            arguments[1] = 9;
            yield b + (() => a + b)();
        }
        print([...g(1, 2)].join(","));
    "#;
    assert_eq!(layouts(source), [(false, true)]);
    assert_eq!(agree(source), ["1,12,23"]);
}

#[test]
fn an_uncaught_throw_in_an_async_function_rejects_its_promise() {
    let lines = agree(
        r#"
        async function failing(n) {
            let sum = 0;
            for (let i = 0; i < n; i++) { sum += await i; if (i === 3) throw new Error("boom" + sum); }
            return sum;
        }
        const jobs = [];
        for (let round = 0; round < 5; round++) jobs.push(failing(10).then(() => "ok", (e) => e.message));
        Promise.all(jobs).then((settled) => print(settled.join(",")));
        "#,
    );
    assert_eq!(lines, ["boom6,boom6,boom6,boom6,boom6"]);
}

#[test]
fn a_generator_that_throws_completes_and_its_caller_unwinds() {
    let lines = agree(
        r#"
        function* thrower(n) { for (let i = 0; i < n; i++) { if (i === 2) throw new Error("stop" + i); yield i; } }
        const out = [];
        for (let round = 0; round < 3; round++) {
            const it = thrower(5);
            out.push(it.next().value, it.next().value);
            try { it.next(); } catch (e) { out.push(e.message); }
            out.push(JSON.stringify(it.next()));
        }
        function* once() { yield 1; throw new Error("x"); }
        let caught = 0;
        for (let round = 0; round < 50; round++) {
            try { for (const v of once()) { } } catch (e) { caught++; }
        }
        print(out.join(" ") + "|" + caught);
        "#,
    );
    let round = r#"0 1 stop2 {"done":true}"#;
    assert_eq!(lines, [format!("{round} {round} {round}|50")]);
}
