//! Stores that meet a setter, as the lane activates it (`setters::lane_store_setter`),
//! against Node's output.
use crate::test_host::printed;

/// A setter's store yields the value stored whatever the setter returns, through plain,
/// compound and logical assignments; throws, recursion, strict and sloppy getter-only
/// stores, an inherited setter's receiver, a redefinition mid-loop, a proxy up the chain,
/// closures, `arguments`, static and primitive receivers, and generator and async callers
/// keep their semantics.
#[test]
fn setter_stores_match_node() {
    let source = r#"
const o = []; let log = [];
class Box { constructor() { this._v = 0; } get v() { return this._v; } set v(x) { this._v = x & 0xff; return "ignored"; } }
class Sub extends Box { set w(x) { this._w = x; return { obj: 1 }; } }
const b = new Sub();
function run(n) { let last; for (let i = 0; i < n; i++) { last = (b.v = i); b.w = i; } return last; }
o.push(run(5000), b._v, b._w);
// assignment expression value, chained, compound, in conditions
let x; x = b.v = 1000; o.push(x, b.v, (b.v += 5), b.v, (b.v++), b.v, b.v ||= 7, b.v &&= 300, b.v);
// setter that throws / recursion / deep
const t = { set a(v) { if (v > 4990) throw new RangeError("big" + v); this._a = v; }, set r(v) { if (v > 0) this.r = v - 1; else this._r = "done"; } };
function thr(n) { try { for (let i = 0; i < n; i++) t.a = i; } catch (e) { return e.message + ":" + t._a; } }
o.push(thr(6000)); t.r = 50; o.push(t._r);
// strictness: getter only, sloppy vs strict
const g = { get q() { return 1; } };
function sloppy(n) { for (let i = 0; i < n; i++) g.q = i; return g.q; }
function strict(n) { "use strict"; try { for (let i = 0; i < n; i++) g.q = i; } catch (e) { return e.constructor.name; } }
o.push(sloppy(3000), strict(3000));
// receiver is this for inherited setter; primitives; proxies in chain; defineProperty setters; setter replaced mid-loop
const proto = { set p(v) { this.own = v; } }; const child = Object.create(proto);
function inh(n) { for (let i = 0; i < n; i++) child.p = i; return child.own + ":" + Object.keys(child).join() + ":" + ("own" in proto); }
o.push(inh(3000));
const d = {}; let calls = 0;
Object.defineProperty(d, "k", { set(v) { calls++; }, configurable: true });
function redef(n) { for (let i = 0; i < n; i++) { d.k = i; if (i === 1500) Object.defineProperty(d, "k", { value: -1, writable: true }); } return calls + ":" + d.k; }
o.push(redef(3000));
const px = Object.create(new Proxy({}, { set(t, k, v, r) { log.push(k); return Reflect.set(t, k, v, r); } }));
function viaProxy(n) { for (let i = 0; i < n; i++) px.z = i; return log.length + ":" + px.z; }
o.push(viaProxy(50));
// setter with closures / arguments / default params / arrow receiver / async / generator callers
const c = { set m(v) { const f = () => this._m = v * 2; f(); }, set n(v = 5) { this._n = arguments.length + ":" + v; } };
function clos(n) { for (let i = 0; i < n; i++) { c.m = i; c.n = i; } return c._m + ":" + c._n; }
o.push(clos(3000));
function* gen() { for (let i = 0; i < 3; i++) { b.v = i; yield b.v; } }
o.push([...gen()].join());
(async () => { for (let i = 0; i < 2000; i++) { b.v = i; if (i % 500 === 0) await null; } return b.v; })().then(v => { o.push(v); print(o.join("|")); });
// setter on class static, symbol-keyed excluded, number receiver
class S { static set s(v) { S._s = v; } } function st(n) { for (let i = 0; i < n; i++) S.s = i; return S._s; } o.push(st(3000));
Object.defineProperty(Number.prototype, "ns", { set(v) { "use strict"; log.push(typeof this); }, configurable: true }); function prim(n) { for (let i = 0; i < n; i++) (5).ns = i; return log[log.length - 1]; } o.push(prim(100));
"#;
    assert_eq!(printed(source, "setters.js"), ["4999|135|4999|1000|232|237|237|237|238|238|300|44|big4991:4990|done|1|TypeError|2999:own:false|1501:2999|1:49|5998:1:2999|0,1,2|2999|number|207"]);
}
