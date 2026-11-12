//! A `with` statement's scope chain is data on the lane stack while a frameless activation
//! runs it (`LaneStack::with_objects`), and in the record of a suspended frame between runs
//! (`GeneratorFrame::with_objects`): entering, leaving and returning through a `with` in a lane
//! run agree with the interpreter's.
use super::window_steps::tests::agree;

#[test]
fn frameless_activations_enter_and_leave_with_scopes_as_the_interpreter_does() {
    let lines = agree(
        r#"
        function plain(o) { with (o) { } return 1; }
        function leaving(o) { for (let i = 0; i < 3; i++) { with (o) { if (i === 1) break; } } return 2; }
        function nested(o, p) { with (o) { with (p) { } } return 3; }
        let sum = 0;
        for (let i = 0; i < 200; i++) sum += plain({ x: i }) + leaving({}) + nested({}, {});
        try { plain(null); } catch (e) { sum += e.constructor === TypeError ? 1000 : 0; }
        print(String(sum));
        "#,
    );
    assert_eq!(lines, ["2200"]);
}

#[test]
fn a_generator_keeps_its_with_scope_across_yields() {
    let lines = agree(
        r#"
        function* counting(o) { with (o) { yield 1; yield 2; } yield 3; }
        const out = [];
        for (let round = 0; round < 20; round++) for (const x of counting({ a: round })) out.push(x);
        print(out.length + ":" + out.slice(0, 3).join(","));
        "#,
    );
    assert_eq!(lines, ["60:1,2,3"]);
}
