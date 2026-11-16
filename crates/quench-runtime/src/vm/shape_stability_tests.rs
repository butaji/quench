//! An object moved out of a shape that compiled code holds past a call (`Vm::shape_is_stable`,
//! `lane_region::kept_shapes`) is seen at the code's next use: each way a shape can be left,
//! from a callee and from outside the compiled function, ends as the interpreter's run does.

use crate::vm::test_oracle::{assert_tier_agrees, Expect, Tier};

/// A program whose hot `read` reads `a.x`, calls `f(a)`, then reads `a.y` (and `a.z`, which no
/// object has until the departure) and stores `a.x`; the one object that departs its shape does
/// so at iteration 15000, in `departure` (a callee `f` of `read`, or a statement between calls).
fn program(callee: &str, between: &str) -> String {
    format!(r#"
     (function () {{
      function Pt(x, y) {{ this.x = x; this.y = y; }}
      function read(a, f) {{ const x = a.x; f(a); a.x = x + 1; return x + (a.y | 0) + (a.z | 0) + a.x; }}
      const pts = [];
      for (let i = 0; i < 8; i++) pts.push(new Pt(i, i + 1));
      const same = (a) => a;
      const leave = {callee};
      let t = 0;
      for (let i = 0; i < 20000; i++) {{
        if (i === 15000) {{ {between} }}
        t += read(pts[i & 7], i === 15000 ? leave : same);
      }}
      globalThis.answer = t;
     }})();
    "#)
}

/// Each departure from a callee: an added property, a deleted one, an accessor, a new prototype
/// holding `z`, a frozen object.
const FROM_CALLEE: &[(&str, &str)] = &[
    ("add", "(a) => { a.z = 5; }"),
    ("delete", "(a) => { delete a.y; }"),
    ("accessor", "(a) => { Object.defineProperty(a, 'y', { get() { return 40; }, configurable: true }); }"),
    ("prototype", "(a) => { Object.setPrototypeOf(a, { z: 7 }); }"),
    ("freeze", "(a) => { Object.freeze(a); }"),
];

#[test]
fn an_object_leaving_a_held_shape_from_a_callee_ends_as_the_interpreter_does() {
    for (name, callee) in FROM_CALLEE {
        eprintln!("departure from a callee: {name}");
        assert_tier_agrees(&program(callee, ""), Tier::Full, Expect::ENGAGED);
        assert_tier_agrees(&program(callee, ""), Tier::UJit, Expect::ENGAGED);
    }
}

#[test]
fn an_object_leaving_a_held_shape_from_outside_ends_as_the_interpreter_does() {
    for (name, between) in [("add", "pts[0].z = 5;"), ("delete", "delete pts[0].y;"),
        ("accessor", "Object.defineProperty(pts[0], 'y', { get() { return 40; }, configurable: true });"),
        ("prototype", "Object.setPrototypeOf(pts[0], { z: 7 });"), ("freeze", "Object.freeze(pts[0]);")] {
        eprintln!("departure from outside: {name}");
        assert_tier_agrees(&program("same", between), Tier::Full, Expect::ENGAGED);
        assert_tier_agrees(&program("same", between), Tier::UJit, Expect::ENGAGED);
    }
}
