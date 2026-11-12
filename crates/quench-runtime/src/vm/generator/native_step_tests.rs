//! Generators and functions the lane runs over records, where the step is a native's (`Array.from`),
//! the function reads what a window must carry (`this` through an arrow, its own name, an `eval`),
//! or the script itself awaits: each runs as a window and equals the interpreter's.
use super::window_steps::tests::{agree, run};

#[test]
fn a_generator_stepped_from_native_code_makes_no_frame() {
    // `Array.from` steps the generator as the native `next`: a lane run rooted at its record.
    let (lines, made) = run(
        r#"
        function* range(n) { for (let i = 0; i < n; i++) yield i; }
        let total = 0;
        for (let round = 0; round < 50; round++) total += Array.from(range(10)).length;
        const it = range(2);
        print([total, JSON.stringify(Array.from(it)), JSON.stringify(it.next())].join(" "));
        "#,
    );
    assert_eq!(lines, ["500 [0,1] {\"done\":true}"]);
    assert_eq!(made, 0, "a native step made a frame");
}

#[test]
fn native_steps_throw_and_return_through_a_generators_handlers() {
    let lines = agree(
        r#"
        const out = [];
        function* g() { try { yield 1; yield 2; } catch (e) { out.push("caught " + e); yield 3; } finally { out.push("fin"); } }
        const a = g(); a.next();
        out.push(JSON.stringify(a.throw("x")), JSON.stringify(a.next()), JSON.stringify(a.next()));
        const b = g(); b.next();
        out.push(JSON.stringify(b.return(5)), JSON.stringify(b.next()));
        try { g().throw("early"); } catch (e) { out.push(e); }
        print(out.join(" "));
        "#,
    );
    assert_eq!(
        lines,
        [r#"caught x fin {"value":3,"done":false} {"done":true} {"done":true} fin {"value":5,"done":true} {"done":true} early"#]
    );
}
