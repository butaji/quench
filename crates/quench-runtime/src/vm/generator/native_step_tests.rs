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

#[test]
fn a_parameter_eval_declaring_arguments_raises_at_the_eval_in_any_activation() {
    // The function runs as an ordinary one; only its parameter's `eval` call raises.
    let lines = agree(
        r#"
        function f(a, p = eval("var arguments = 1")) { return a; }
        function* g(a, p = eval("var arguments = 1")) { yield a; }
        const out = [f(1, 2)];
        for (const call of [() => f(1), () => g(1).next()]) {
            try { const r = call(); out.push(r); } catch (e) { out.push(e.constructor.name); }
        }
        out.push(g(5, 6).next().value);
        print(out.join(" "));
        "#,
    );
    assert_eq!(lines, ["1 SyntaxError SyntaxError 5"]);
}

#[test]
fn a_named_function_expression_binds_its_own_name_in_its_window() {
    // The name is a window local the entry initializes with the callee; a store to it leaves the
    // lane, where it is ignored (sloppy) or throws (strict).
    let lines = agree(
        r#"
        const out = [];
        const fact = function f(n) { const k = () => n; return n < 2 ? k() : n * f(n - 1); };
        out.push(fact(6));
        const rebind = function r(n) { const k = () => n; r = 1; return typeof r + k(); };
        out.push(rebind(3));
        const strict = function s(n) { "use strict"; const k = () => n; try { s = 1; } catch (e) { return e.constructor.name + k(); } };
        out.push(strict(2));
        const gen = function* g(n) { const k = () => n; yield typeof g; yield k(); };
        out.push([...gen(4)].join());
        const viaClosure = function c(n) { const k = () => n; return [1].map(() => c.length + k())[0]; };
        out.push(viaClosure(5));
        let total = 0;
        for (let i = 0; i < 500; i++) total += fact(5);
        out.push(total);
        print(out.join("|"));
        "#,
    );
    assert_eq!(lines, ["720|function3|TypeError2|function,4|6|60000"]);
}

#[test]
fn iteration_copies_of_a_window_environment_share_its_dynamic_scope() {
    // A loop's per-iteration environment copy under a `with` statement, in an ordinary function
    // and a generator, whose closures read the object's binding and their own iteration's.
    let lines = agree(
        r#"
        function ordinary(o) { const fs = []; with (o) { for (let i = 0; i < 3; i++) fs.push(() => i + x); } return fs.map((f) => f()).join(); }
        function* generating(o) { const fs = []; with (o) { for (let i = 0; i < 3; i++) { fs.push(() => i + x); yield i; } } yield fs.map((f) => f()).join(); }
        const o = { x: 10 };
        let last;
        for (const v of generating(o)) last = v;
        print([ordinary(o), last].join("|"));
        "#,
    );
    assert_eq!(lines, ["10,11,12|10,11,12"]);
}

#[test]
fn an_async_arrow_reads_its_lexical_this_across_awaits() {
    // The arrow's receiver is its environment's, read again at each resumption: a `super()` that
    // ran while it was suspended has initialized it.
    let lines = agree(
        r#"
        const out = [];
        const holder = { v: 7, run() { const a = async (k) => { const first = this.v; await k; await k; return first + this.v + k; }; return a(1); } };
        holder.run().then((v) => out.push(v));
        class A {}
        class B extends A {
            constructor() {
                const a = async () => { await 0; return typeof this; };
                const pending = a();
                super();
                this.pending = pending;
            }
        }
        new B().pending.then((v) => { out.push(v); print(out.join(" ")); });
        "#,
    );
    assert_eq!(lines, ["15 object"]);
}
