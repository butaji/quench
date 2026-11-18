//! A `with` statement's scope chain is data on the lane stack while a frameless activation
//! runs it (`LaneStack::with_objects`), and in the record of a suspended frame between runs
//! (`FrameRecord::with_objects`): entering, leaving and returning through a `with` in a lane
//! run agree with the interpreter's. A suspending function that resolves names through `with`
//! objects, binding sites or a direct eval's dynamic bindings resumes in its window
//! (`CallPlan::suspended_environment`, `scoped_names`), as the interpreter's frame does.
use super::window_steps::tests::{agree, layouts, run};

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


#[test]
fn functions_resolving_names_through_a_scope_ride_windows_unless_they_copy_their_environment() {
    let layouts = layouts(
        "function* scoped(o) { with (o) { yield a; } }
         function* evaluating() { eval('var v = 1'); yield v; }
         function* looping(o) { with (o) { for (let i = 0; i < 2; i++) yield () => i; } }
         async function awaiting(o) { with (o) { return (await a) + (await b); } }",
    );
    assert_eq!(layouts, [(false, true), (false, true), (false, false), (false, true)]);
}

#[test]
fn a_generator_in_a_with_statement_resumes_without_frames() {
    // The names its body reads and the one it stores resolve in the window: the `with` object
    // that has them answers, and the chain moves into the record at each `yield`.
    let source = |rounds: u32| {
        format!(
            r#"
            function* walking(o) {{ with (o) {{ yield a; yield b; a = 10; yield a + b; }} yield 0; }}
            let sum = 0;
            for (let round = 0; round < {rounds}; round++) {{
                const o = {{ a: round, b: 1 }};
                for (const x of walking(o)) sum += x;
                sum += o.a;
            }}
            print(String(sum));
            "#
        )
    };
    let (lines, made) = run(&source(50));
    assert_eq!(lines, ["2325"]);
    assert_eq!(made, 0, "a resume made a frame");
    agree(&source(50));
}

#[test]
fn an_async_function_in_a_with_statement_awaits_in_its_window() {
    let lines = agree(
        r#"
        async function adding(o) { with (o) { return (await a) + (await b); } }
        let total = 0;
        const pending = [];
        for (let i = 0; i < 20; i++) pending.push(adding({ a: i, b: 2 }).then((value) => { total += value; }));
        Promise.all(pending).then(() => print(String(total)));
        "#,
    );
    assert_eq!(lines, ["230"]);
}

#[test]
fn a_closure_made_in_a_with_statement_keeps_its_scope_after_the_generator_resumes() {
    let source = r#"
        function* making(o) { with (o) { yield () => a; yield 2; } }
        const o = { a: 1 };
        const made = [];
        for (const x of making(o)) if (typeof x === "function") made.push(x);
        o.a = 7;
        print(String(made[0]()));
    "#;
    assert_eq!(agree(source), ["7"]);
    let (_, frames) = run(source);
    assert_eq!(frames, 0, "a resume made a frame");
}

#[test]
fn a_var_eval_introduced_reads_after_a_yield_through_the_activation() {
    // The eval runs in the window; the dynamic binding it introduces lives in the record, which
    // the later windows read by name.
    let source = |rounds: u32| {
        format!(
            r#"
            function* evaluating() {{ eval("var v = 40"); yield v; yield v + 1; yield v + 2; }}
            let sum = 0;
            for (let round = 0; round < {rounds}; round++) for (const x of evaluating()) sum += x;
            print(String(sum));
            "#
        )
    };
    assert_eq!(agree(&source(30)), ["3690"]);
    let (_, frames) = run(&source(30));
    assert_eq!(frames, 0, "an eval step made a frame");
}

#[test]
fn an_ordinary_function_with_locals_and_a_with_statement_runs_as_an_environment_activation() {
    // It makes a closure over a local and resolves names through the `with` object, which its
    // activation's chain holds (`LaneStack::with_objects`): the window needs no frame.
    let source = r#"
        function pick(o, k) { let total = 0; const add = (n) => total += n; with (o) { add(a); add(k); } return total; }
        let sum = 0;
        for (let i = 0; i < 300; i++) sum += pick({ a: i }, 2);
        print(String(sum));
    "#;
    assert_eq!(agree(source), ["45450"]);
    let program = crate::Engine::specialize(source, "scoped-activation.js").unwrap();
    let pick = program
        .compiled_functions()
        .find(|function| function.name.is_some_and(|atom| program.atoms[atom as usize].to_string() == "pick"))
        .expect("the function is in the program");
    assert!(pick.call_plan(&program).environment_activation);
}

#[test]
fn an_ordinary_function_reading_implicit_bindings_stays_a_frame() {
    // A direct eval reads the activation's implicit bindings, which a frameless activation has
    // no record to keep.
    let program = crate::Engine::specialize("function f(o) { let t = 0; with (o) { t = eval('a'); } return () => t; }", "eval-frame.js").unwrap();
    let f = program
        .compiled_functions()
        .find(|function| function.name.is_some_and(|atom| program.atoms[atom as usize].to_string() == "f"))
        .expect("the function is in the program");
    assert!(!f.call_plan(&program).environment_activation);
}
