//! Record slots hold tagged words (`ValueArena::float` is never set), so every program computes what
//! the interpreter computes in each execution mode.

use super::Vm;

/// Programs whose fields move between int32, double, string and object values, read by the
/// interpreter and by compiled loops, and pass through every way a property is copied or listed.
const CORPUS: &[(&str, &str)] = &[
    ("transitions", r#"
        function mk(i) { return { a: i, b: 1, c: 2 }; }
        var objs = []; for (var i = 0; i < 50; i++) objs.push(mk(i));
        function sum() { var s = 0; for (var i = 0; i < objs.length; i++) { s += objs[i].a + objs[i].b; } return s; }
        var out = [sum()];
        for (var i = 0; i < objs.length; i++) objs[i].a = i + 0.5;
        out.push(sum());
        for (var k = 0; k < 300; k++) sum();
        out.push(sum());
        objs[3].a = "x";
        out.push(String(objs[3].a) + "," + objs[4].a);
        objs[5].a = { z: 1 };
        out.push(JSON.stringify(objs[2]) + JSON.stringify(objs[5]));
        globalThis.answer = out.join("|");
    "#),
    ("identity", r#"
        var o = { x: 1, y: 2, z: 3 };
        o.x = 1.5; o.x = 2; o.y = 4.5; o.y = 4; o.z = -0;
        var m = new Map(); m.set(o.y, "four");
        var arr = [10, 20, 30, 40, 50];
        var big = { p: 1, q: 2, r: 3 }; big.p = 0.5; big.p = 2147483648; big.q = NaN;
        globalThis.answer = [o.x === 2, Object.is(o.x, 2), m.get(4), arr[o.y], arr[o.y - 3], 1 / o.z, Object.is(o.z, -0),
            JSON.stringify(o), Object.keys(o).join(), Object.assign({}, o).y, ({ ...o }).x, big.p + 1, isNaN(big.q),
            Object.entries(o).join(";"), [o.y].indexOf(4), new Set([o.y, 4]).size].join("|");
    "#),
    ("shape edits", r#"
        var p = { a: 1.5, b: 2, c: 3 }; delete p.b; p.d = 4.25;
        var q = { a: 1, b: 2, c: 3 }; q.a = 0.5; Object.freeze(q);
        var r = { a: 1, b: 2, c: 3 }; r.a = 0.25; delete r.c; r.c = 7;
        var dict = {}; for (var i = 0; i < 200; i++) dict["k" + i] = i + 0.5; delete dict.k10;
        var s = { a: 1, b: 2, c: 3 }; s.a = 1.5; Object.defineProperty(s, "b", { get() { return 9; } });
        globalThis.answer = [JSON.stringify(p), JSON.stringify(q), JSON.stringify(r), Object.keys(dict).length, dict.k199,
            dict.k11, Object.isFrozen(q), s.b, s.a].join("|");
    "#),
    ("collection", r#"
        var keep = [];
        for (var r = 0; r < 20; r++) {
            var t = [];
            for (var i = 0; i < 2000; i++) t.push({ x: i + 0.5, y: i, z: r });
            keep.push(t[r * 10]);
        }
        var s = 0; for (var i = 0; i < keep.length; i++) s += keep[i].x + keep[i].y + keep[i].z;
        globalThis.answer = String(s);
    "#),
    ("n-body", r#"
        function body(x, vx, m) { return { x: x, vx: vx, m: m, y: 0 }; }
        var a = body(0, 0, 1), b = body(1, 0, 2);
        function step(n) {
            for (var i = 0; i < n; i++) {
                var dx = a.x - b.x, mag = 0.01 / (dx * dx + 0.1);
                a.vx -= dx * b.m * mag; b.vx += dx * a.m * mag;
                a.x += 0.01 * a.vx; b.x += 0.01 * b.vx;
            }
        }
        for (var k = 0; k < 400; k++) step(50);
        globalThis.answer = [a.x, a.vx, b.x, b.vx].join(",");
    "#),
];

/// The program's `answer`, run in `mode`.
fn run(source: &str, mode: crate::ExecutionMode) -> String {
    let program = crate::Engine::specialize(source, "record-slots.js").unwrap();
    let mut vm = Vm::new(crate::test_host::TestHost::default());
    vm.set_execution_mode(mode);
    vm.programs.enable_js_lane_methods();
    vm.execute(&program).unwrap();
    let answer = vm.intern_atom("answer");
    let value = vm.own_property(vm.realm.globals, answer).unwrap();
    vm.heap.string(value).unwrap().to_string()
}

#[test]
fn every_mode_computes_what_the_interpreter_computes() {
    for (name, source) in CORPUS {
        let expected = run(source, crate::ExecutionMode::Interpreted);
        assert_eq!(run(source, crate::ExecutionMode::Compiled), expected, "{name} compiled");
    }
}

/// A double-field dot product loop run hot: its own-field reads take tagged doubles, and a string written
/// to a double field mid-run leaves the loop's compiled body, so the later iterations coerce the string
/// as the interpreter does.
#[test]
fn a_double_field_loop_survives_a_string_written_mid_run() {
    let source = r#"
        var a = { x: 1.5, y: 2.5, z: 3.5 }, b = { x: 2, y: 0.5, z: 4 };
        function dot(n) {
            var s = 0;
            for (var i = 0; i < n; i++) { s += a.x * b.x + a.y * b.y + a.z * b.z; }
            return s;
        }
        for (var k = 0; k < 300; k++) dot(10);
        var out = [dot(1000)];
        a.x = "3";
        out.push(dot(500));
        globalThis.answer = out.join("|");
    "#;
    let expected = run(source, crate::ExecutionMode::Interpreted);
    assert_eq!(expected, "18250|10625");
    assert_eq!(run(source, crate::ExecutionMode::Compiled), expected, "compiled");
}
