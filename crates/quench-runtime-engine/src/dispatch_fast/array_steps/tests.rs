#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::Engine;
use quench_runtime::test_host::TestHost;
use quench_runtime::vm::Vm;

const SOURCE: &str = r#"var gc = typeof $262 !== "undefined" ? $262.gc : typeof globalThis.gc === "function" ? globalThis.gc : function () {};
var out = [];
out.push([1, 2, 3, 4].reduce(function (p, x, i, a) { return p + x * i + a.length + arguments.length; }, 10));
out.push([1, 2, 3, 4].reduceRight(function (p, x, i) { return p + "," + x + i; }));
out.push([, , 5, , 7].reduce(function (p, x, i) { return p + ":" + x + "@" + i; }));
out.push([, , 5, , 7].reduceRight(function (p, x, i) { return p + ":" + x + "@" + i; }));
Array.prototype[1] = "p";
out.push([0, , 2].reduce(function (p, x) { return p + x; }, ""));
delete Array.prototype[1];
try { [].reduce(function (a) { return a; }); } catch (e) { out.push(e.constructor.name); }
try { [, ,].reduceRight(function (a) { return a; }); } catch (e) { out.push(e.constructor.name); }
out.push([].reduce(function (a) { return a; }, "init"), [7].reduce(function (a) { return a + 100; }));
out.push(Array.prototype.reduce.call({ length: 3, 0: "a", 2: "c" }, function (p, x, i) { return p + x + i; }, ">"));
out.push(Array.prototype.reduceRight.call("abc", function (p, x) { return p + x.toUpperCase(); }, ""));
out.push(new Int8Array([1, 2, 3]).reduce(function (p, x) { return p * 10 + x; }));
var m = [1, 2, 3, 4, 5];
out.push(m.reduce(function (p, x, k) { if (k === 0) m.push(99); if (k === 1) m.length = 3; return p + x; }, 0));
var t;
[1].reduce(function () { "use strict"; t = this; return 0; }, 0);
out.push(t === undefined);
var objs = []; for (var i = 0; i < 60; i++) objs.push({ v: i });
var kept = objs.reduce(function (acc, o, k) { var v = o.v; objs[k] = null; if (k % 7 === 0) gc(); var junk = []; for (var j = 0; j < 30; j++) junk.push({ j: j }); return acc.concat([{ w: v }]); }, []);
out.push(kept.length, kept[59].w);
try { [1, 2, 3].reduce(function (p, x) { if (x === 3) throw new RangeError("r" + p); return p + x; }); } catch (e) { out.push(e.message); }
out.push([[1, 2], [3]].reduce(function (a, b) { return a.concat(b.reduce(function (x, y) { return x + y; }, 0)); }, []).join("."));
var stacks = [1, 2, 3].reduce(function f(p, x) { return p + (new Error("s").stack.indexOf("f") >= 0 ? x : 0); }, 0);
out.push(stacks);
out.push([1, 2, 3].reduce(function (p, x) { return p + this; }.bind(5), 0), [1, 2, 3].reduce(Math.max));
var result = out.join("|");
"#;

/// `reduce` and `reduceRight` over dense arrays step their callbacks' lane activations
/// (`begin_array_iteration_in_lane`) with the accumulator first: initial values and their
/// absence, holes and inherited elements, array-likes, strings and typed arrays on the native's
/// loop, the length read once, a strict callback's `this`, collections inside callbacks,
/// throws, nesting, callbacks that become frames, and bound and native callbacks behave as
/// Node's (`node --expose-gc`), specialized or not.
#[test]
fn lane_reducers_keep_reduce_semantics() {
    let source = SOURCE;
    for specialized in [false, true] {
        let mut vm = Vm::new(TestHost::default().with_test262());
        let program = if specialized {
            Engine::specialize(source, "lane-reducers.js")
        } else {
            Engine::compile(quench_runtime::ExecutionRequest { source, name: "lane-reducers.js", kind: quench_runtime::SourceKind::Script })
        }
        .unwrap();
        vm.execute(&program).unwrap();
        let name = vm.intern_atom("result");
        let value = vm.own_property(vm.realm.globals, name).unwrap();
        assert_eq!(vm.to_string(&program, value).unwrap(), "62|4,32,21,10|5:7@4|7:5@2|0p2|TypeError|TypeError|init|7|>a0c2|CBA|123|6|true|60|59|r3|3.3|6|15|NaN");
    }
}

const PLAIN_CALLBACKS: &str = r#"var gc = typeof $262 !== "undefined" ? $262.gc : typeof globalThis.gc === "function" ? globalThis.gc : function () {};
var out = [];
var a = [1, 2, 3, 4];
out.push(a.map(x => x * 2).join(","));
out.push(a.filter(x => x % 2 === 0).join(","));
var s = 0; a.forEach(x => { s += x; }); out.push(s);
out.push(a.reduce((t, v) => t + v, 0), a.reduce((t, v) => t + v), a.reduceRight((t, v) => t + "" + v, ""));
out.push(a.some(x => x > 3), a.every(x => x > 0), a.find(x => x > 2), a.findIndex(x => x > 2));
function inner(b) { return b.map(y => y + 1).reduce((t, v) => t + v, 0); }
out.push(a.map(x => inner([x, x])).join(","));
var m = [1, 2, 3, 4, 5];
out.push(m.map((x, i) => { if (i === 0) m.length = 2; return x; }).join(","));
try { a.map(x => { if (x === 3) throw new RangeError("m" + x); return x; }); } catch (e) { out.push(e.message); }
out.push(a.map(x => { var junk = []; for (var j = 0; j < 50; j++) junk.push({ j: j }); gc(); return junk.length + x; }).join(","));
var o = {}, t;
[1].forEach(function () { "use strict"; t = this; }, o);
out.push(t === o);
function id(v) { return v; }
out.push(a.map(x => id(x)).join(","));
var result = out.join("|");
"#;

/// Callbacks that keep nothing (arrows, strict functions) step as plain activations returning
/// through the array steps' trampoline (`array_step_returned`): every method's step, nesting
/// over plain frames, a receiver shortened mid-iteration (the native's loop takes the holes),
/// a throw, collections inside callbacks, `thisArg`, and a returning call (which spills the
/// header into the step's record) behave as the specification says (expected string worked by
/// hand, not yet checked against `node --expose-gc`), specialized or not.
#[test]
fn lane_plain_array_steps_keep_semantics() {
    for specialized in [false, true] {
        let mut vm = Vm::new(TestHost::default().with_test262());
        let program = if specialized {
            Engine::specialize(PLAIN_CALLBACKS, "lane-plain-steps.js")
        } else {
            Engine::compile(quench_runtime::ExecutionRequest { source: PLAIN_CALLBACKS, name: "lane-plain-steps.js", kind: quench_runtime::SourceKind::Script })
        }
        .unwrap();
        vm.execute(&program).unwrap();
        let name = vm.intern_atom("result");
        let value = vm.own_property(vm.realm.globals, name).unwrap();
        assert_eq!(
            vm.to_string(&program, value).unwrap(),
            "2,4,6,8|2,4|10|10|10|4321|true|true|3|2|4,6,8,10|1,2,,,|m3|51,52,53,54|true|1,2,3,4"
        );
    }
}
