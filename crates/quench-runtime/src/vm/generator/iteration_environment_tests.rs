//! Per-iteration `let` bindings that closures capture, across yields and awaits: the window's
//! `CloneEnv` copies the environment its closures made (`lane_clone_window_env`), the closures of
//! earlier iterations keeping theirs, and no resume makes a frame. Compiled runs agree with the
//! interpreter's.
use super::window_steps::tests::{agree, run};

const GENERATOR: &str = r#"
    function* counting(n) {
        const fns = [];
        for (let i = 0; i < n; i++) {
            fns.push(() => i);
            yield i;
            i *= 2;
        }
        yield fns.map((f) => f()).join("|");
    }
    print([...counting(4)].join(","));
"#;

const ASYNC: &str = r#"
    async function counting(n) {
        const fns = [];
        for (let i = 0; i < n; i++) {
            fns.push(() => i);
            await i;
            i *= 2;
        }
        return fns.map((f) => f()).join("|");
    }
    counting(4).then((joined) => print(joined));
"#;

#[test]
fn a_generator_with_per_iteration_closures_agrees_with_the_interpreter() {
    assert_eq!(agree(GENERATOR), ["0,1,3,0|2|6"]);
}

#[test]
fn an_async_function_with_per_iteration_closures_agrees_with_the_interpreter() {
    assert_eq!(agree(ASYNC), ["0|2|6"]);
}

#[test]
fn per_iteration_environment_copies_across_suspensions_make_no_frame() {
    let (lines, made) = run(GENERATOR);
    assert_eq!(lines, ["0,1,3,0|2|6"]);
    assert_eq!(made, 0, "a generator resume made a frame");
    let (lines, made) = run(ASYNC);
    assert_eq!(lines, ["0|2|6"]);
    assert_eq!(made, 0, "an async resume made a frame");
}
