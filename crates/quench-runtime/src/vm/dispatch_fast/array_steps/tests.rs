use crate::Engine;
use crate::test_host::TestHost;
use crate::vm::Vm;

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
/// (`lane_begin_array_iteration`) with the accumulator first: initial values and their
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
            Engine::compile(crate::ExecutionRequest { source, name: "lane-reducers.js", kind: crate::SourceKind::Script })
        }
        .unwrap();
        vm.execute(&program).unwrap();
        let name = vm.intern_atom("result");
        let value = vm.own_property(vm.realm.globals, name).unwrap();
        assert_eq!(vm.to_string(&program, value).unwrap(), "62|4,32,21,10|5:7@4|7:5@2|0p2|TypeError|TypeError|init|7|>a0c2|CBA|123|6|true|60|59|r3|3.3|6|15|NaN");
    }
}
