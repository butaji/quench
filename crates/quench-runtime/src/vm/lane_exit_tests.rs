//! Operations the lane answers in place that once left it: each runs often enough for the
//! in-place path, and its output is what Node prints for the same source.
use super::dispatch_fast::super_chain::tests::printed;

/// A number, boolean, bigint or symbol read of a name its prototype chain lacks is `undefined`;
/// a prototype that gains the name, a getter that sees a primitive `this`, or a changed chain
/// are seen at once.
#[test]
fn primitive_field_reads_follow_their_prototypes() {
    let source = r#"
const vals = [1.5, 0, true, false, 10n, Symbol("q"), "str", 5];
function f(p) { return p.isColor ? 1 : 0; }
log(rep(() => vals.map(f).join()));
Number.prototype.isColor = "num";
log(vals.map(f).join());
delete Number.prototype.isColor;
Object.defineProperty(Number.prototype, "isColor", { get() { "use strict"; return typeof this; }, configurable: true });
log(vals.map(f).join());
delete Number.prototype.isColor;
let calls = 0;
for (let i = 0; i < 4000; i++) { if (i === 2000) Boolean.prototype.isColor = 1; calls += f(true); }
log(calls);
delete Boolean.prototype.isColor;
const toString = Number.prototype.toString; delete Number.prototype.toString;
function t(p) { return typeof p.toString; }
log(rep(() => t(1.5), 50));
Number.prototype.toString = toString;
try { rep(() => f(null), 5); } catch (e) { log(e.constructor.name); }
print(out.join(" | "));
"#;
    assert_eq!(
        printed(source, "primitive-fields.js"),
        ["0,0,0,0,0,0,0,0 | 1,1,0,0,0,0,0,1 | 1,1,0,0,0,0,0,1 | 2000 | function | TypeError"]
    );
}

/// A for-in over shape-listed objects, shadowed or deleted keys, keys added mid-loop, arrays,
/// strings, `null`, proxies and a polluted `Object.prototype` visits the keys Node does, in its
/// order.
#[test]
fn for_in_steps_visit_the_keys_node_visits() {
    let source = r#"
function keys(o) { const r = []; for (const k in o) r.push(k); return r.join(","); }
const base = { a: 1, b: 2, c: 3 }; const d = Object.create(base); d.b = 9; d.z = 1;
Object.defineProperty(d, "c", { value: 1, enumerable: false });
function del(o) { const r = []; for (const k in o) { r.push(k); delete o.b; delete o.c; } return r.join(); }
function add(o) { const r = []; for (const k in o) { r.push(k); o[k + "x"] = 1; } return r.join(); }
const px = new Proxy({ a: 1, b: 2 }, { getOwnPropertyDescriptor(t, k) { return Reflect.getOwnPropertyDescriptor(t, k); } });
function nested() { let r = ""; for (const a in { x: 1, y: 2 }) { for (const b in [1, 2]) { if (b === "1") continue; r += a + b; } if (a === "y") break; } return r; }
log(rep(() => keys({ b: 1, 10: 1, 2: 1, a: 1, "-1": 1 })), rep(() => keys(d)), rep(() => del({ a: 1, b: 2, c: 3, d: 4 })), rep(() => add({ a: 1, b: 2 })));
log(rep(() => keys([5, 6])), rep(() => keys("ab")), rep(() => keys(null)), rep(() => keys(undefined)), rep(() => keys(7)), rep(nested));
log(keys(px));
Object.prototype.polluted = 1; log(rep(() => keys({ a: 1 }))); delete Object.prototype.polluted;
print(out.join(" | "));
"#;
    assert_eq!(
        printed(source, "for-in-steps.js"),
        ["2,10,b,a,-1 b,z,a a,d a,b | 0,1 0,1    x0y0 | a,b | a,polluted"]
    );
}

/// Natives the lane runs in place (`Map` iterators, `toString` of strings and functions,
/// legacy `RegExp` statics, an iterator's close) give Node's values and throw where it throws.
#[test]
fn in_place_natives_keep_their_results_and_errors() {
    let source = r#"
const m = new Map([[1, "a"], [2, "b"]]);
log(rep(() => [...m.values()].join()), rep(() => [...m.keys()].join()), rep(() => [...m.entries()].join(";")));
class MM extends Map {}
log(rep(() => [...new MM([[3, "c"]]).values()].join()));
try { Map.prototype.values.call({}); } catch (e) { log(e.constructor.name); }
try { Map.prototype.values.call(new Set()); } catch (e) { log(e.constructor.name); }
log(rep(() => "abc".toString() + "x".valueOf()), rep(() => String.prototype.toString.call(new String("w"))));
try { String.prototype.toString.call(5); } catch (e) { log(e.constructor.name); }
function foo(a, b) { return a + b; }
log(rep(() => foo.toString(), 50), rep(() => Math.max.toString(), 50), rep(() => foo.bind(null).toString(), 50));
try { Function.prototype.toString.call({}); } catch (e) { log(e.constructor.name); }
/(\d+)-(\w+)/.exec("x 12-ab y");
log(rep(() => RegExp.$1 + RegExp.$2 + RegExp.lastMatch + RegExp["$`"] + RegExp["$'"], 50), RegExp.lastParen);
function* gen() { try { yield 1; yield 2; } finally { log("closed"); } }
for (const x of gen()) { break; }
for (const x of [1, 2, 3]) { break; }
const it = { [Symbol.iterator]() { return { next() { return { value: 1, done: false }; }, return() { log("ret"); return {}; } }; } };
for (const x of it) break;
const it2 = { [Symbol.iterator]() { return { next() { return { value: 1, done: false }; } }; } };
for (const x of it2) break;
const it3 = { [Symbol.iterator]() { return { next() { return { value: 1, done: false }; }, return() { return 5; } }; } };
try { for (const x of it3) break; } catch (e) { log(e.constructor.name); }
print(out.join(" | "));
"#;
    assert_eq!(
        printed(source, "in-place-natives.js"),
        ["a,b 1,2 1,a;2,b | c | TypeError | TypeError | abcx w | TypeError | function foo(a, b) { return a + b; } function max() { [native code] } function () { [native code] } | TypeError | 12ab12-abx  y ab | closed | ret | TypeError"]
    );
}
