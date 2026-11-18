//! A direct eval's functions compile on first call from a source of four megabytes or more
//! (`ParsedProgramShape::stubbing`), and behave as their eager compile does.
use super::tests::result_of;

/// `body` as a JavaScript string literal.
fn js_literal(body: &str) -> String {
    format!("'{}'", body.replace('\\', "\\\\").replace('\'', "\\'").replace('\n', "\\n"))
}

/// What a direct eval of `body` in a derived class's method, which also names a private field,
/// gives: padded past the size from which a direct eval's functions compile on first call, and
/// as the same text with a comment of one line in its place, which compiles them eagerly. The
/// two results join with `|||`.
fn stubbed_and_eager(body: &str) -> String {
    let source = format!(
        "class B {{ constructor() {{ this.tag = 'b'; }} }} \
         class A extends B {{ #p = 7; run(src) {{ try {{ return String(eval(src)); }} catch (e) {{ return e.name; }} }} }} \
         const body = {}; const pad = '/*' + 'x'.repeat(4 << 20) + '*/\\n'; \
         globalThis.result = new A().run(pad + body) + '|||' + new A().run('/**/\\n' + body);",
        js_literal(body)
    );
    result_of(&source)
}

/// Each case's body, and the result both of its compiles give, or none where only their
/// agreement is asked.
const CASES: &[(&str, &str)] = &[
    ("function f(a, b) { return a + b + typeof this; } f(1, 2)", "3undefined"),
    ("function g() { return arguments.length + (new.target === undefined); } g(1, 2) + ',' + new g().constructor.name", "3,g"),
    ("function rec(n) { return n ? n + rec(n - 1) : 0; } rec(5) + ':' + rec.name + rec.length", "15:rec1"),
    ("let k = 1; var v = 2; function sum() { return k + v + typeof run; } k = 10; sum()", "12undefined"),
    ("function mk(n) { return function inner() { return ++n; }; } const a = mk(5); [a(), a(), mk(0)()].join()", "6,7,1"),
    ("const add = (a, b) => { return a + b + k; }; let k = 1; add.name + add(1, 2) + add.length", "add42"),
    ("const mk = () => { let n = 0; return () => { n += 1; return n; }; }; const c = mk(); c(); c() + mk()()", "3"),
    ("const t = () => { return this.tag + typeof arguments + String(new.target); }; t()", "bobjectundefined"),
    ("const p = () => { return this.#p; }; p()", "7"),
    ("const o = (x) => { const inner = (y) => { return () => { return x + y; }; }; return inner(2)(); }; o(1)", "3"),
    ("const n = (x) => { return x; }; let r; try { new n(1); } catch (e) { r = e.name; } r + Object.getOwnPropertyNames(n).join()", "TypeErrorlength,name"),
    ("function ev(x) { return eval('x + 1'); } ev(1)", "2"),
    ("function late() { return typeof later; } let later = 1; late()", "number"),
    ("function boom() { null.x; } let s; try { boom(); } catch (e) { s = e.stack.split('\\n').slice(0, 3).join('|'); } s", ""),
    ("function bad() { let a; let a; } 1", "SyntaxError"),
    ("const bad = () => { return 1 +; }; 1", "SyntaxError"),
    ("function x() { return function eval() {}; } 1", "SyntaxError"),
    ("class C { constructor() { this.v = 1; } m(a) { return a + this.v; } get g() { return 2; } static s() { return 's'; } } const c = new C(); c.m(1) + c.g + C.s() + C.prototype.m.name + C.prototype.m.length", "4sm1"),
    ("class D { m() { return typeof D + (new.target === undefined); } } let r; try { new (new D().m)(); } catch (e) { r = e.name; } new D().m() + r", "functiontrueTypeError"),
    ("let base = 5; class E { add(x) { return base + x; } } base = 6; new E().add(1)", "7"),
    ("class F { 'k k'(a) { return a; } set v(x) { this.x = x; } } Object.getOwnPropertyDescriptor(F.prototype, 'v').set.name + F.prototype['k k'].toString()", "set v'k k'(a) { return a; }"),
    ("class G extends Object { m() { return super.constructor === Object; } } new G().m()", "true"),
    ("class H { m() { return 'H'; } static s() { return 'S'; } get g() { return 'g'; } } class I extends H { m() { const f = () => super.m(); return 'I' + f(); } static s() { return 's' + super.s(); } get g() { return super.g + '!'; } } const i = new I(); i.m() + I.s() + i.g", "IHsSg!"),
    ("function outer() { class J { m() { return 1; } } class K extends J { m() { return super.m() + 1; } } return new K().m(); } outer()", "2"),
    ("var w = 'out'; function dp(a = w, { b } = { b: 2 }, ...r) { var z = 1; return a + b + r.length + z; } const da = (x = w) => { return x; }; dp() + dp('x', { b: 3 }, 1, 2) + da()", "out201x321out"),
];

/// Each case gives the same result stubbed as eager, and the one it names.
#[test]
fn large_direct_evals_compile_functions_on_first_call() {
    for (body, expect) in CASES {
        let both = stubbed_and_eager(body);
        let (stubbed, eager) = both.split_once("|||").unwrap();
        assert_eq!(stubbed, eager, "{body}");
        assert!(expect.is_empty() || stubbed == *expect, "{body}: {stubbed}");
    }
}
