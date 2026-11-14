//! Stores that meet a setter, as the lane activates it (`setters::lane_store_setter`),
//! against Node's output.
use crate::test_host::printed;

/// The stores `setter_stores_match_node` runs.
const SOURCE: &str = r#"
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

/// A setter's store yields the value stored whatever the setter returns, through plain,
/// compound and logical assignments; throws, recursion, strict and sloppy getter-only
/// stores, an inherited setter's receiver, a redefinition mid-loop, a proxy up the chain,
/// closures, `arguments`, static and primitive receivers, and generator and async callers
/// keep their semantics.
#[test]
fn setter_stores_match_node() {
    assert_eq!(printed(SOURCE, "setters.js"), ["4999|135|4999|1000|232|237|237|237|238|238|300|44|big4991:4990|done|1|TypeError|2999:own:false|1501:2999|1:49|5998:1:2999|0,1,2|2999|number|207"]);
}

/// A setter activation that a native call out of the lane made a frame returns the stored
/// value read from the caller's register file, not the setter's: the loop counter survives
/// a getter running `Set.add` and a setter running `Set.forEach` (Node prints 8).
#[test]
fn setter_made_a_frame_by_a_native_reads_the_callers_register() {
    let source = r#"
let cnt = 0;
class OV { constructor() { this.obs = new Set(); }
  get() { if (++cnt > 10000) throw new Error("spin"); this.obs.add(1); return 1; }
  set(v) { this.obs.forEach(x => {}); } }
const A = new OV(), C = new OV();
const o = { get a() { return A.get(); }, set c(v) { C.set(v); } };
function run(n) { let s = 0; for (let i = 0; i < n; i++) { s += o.a; o.c = i; } return s; }
print(run(8) + ":" + cnt);
"#;
    assert_eq!(printed(source, "setter_frame.js"), ["8:8"]);
}

/// A store's setter is cached at its site under the receiver's shape, prototype and the
/// prototype layout epoch (`Vm::cached_field_accessor`): a receiver that gains a field, an own
/// property that shadows the setter, a prototype whose accessor is redefined or deleted or
/// gains a getter-only form, an accessor defined after data stores, `Object.setPrototypeOf`
/// moving the receiver or its prototype, a pair redefined in its slot, a proxy inserted up the
/// chain and a frozen receiver each leave the store Node's (the same program prints the same
/// line under Node 24).
#[test]
fn cached_setter_stores_follow_invalidation_like_node() {
    const CACHED: &str = r#"const out = [];
const log = (...a) => out.push(a.join(","));
class A { constructor(){ this.f = 0; } get v(){ return this.f; } set v(x){ this.f = x + 1; } }
function store(o, n) { for (let i = 0; i < n; i++) o.v = i; return o.f; }
const a = new A(); log(store(a, 3000));
// shape transition after cached store: receiver gains a field
a.extra = 1; log(store(a, 300));
// own data property shadows the setter
const b = new A(); store(b, 3000); Object.defineProperty(b, "v", { value: 5, writable: true, configurable: true }); log(store(b, 10), b.v);
// prototype mutation: setter replaced by another accessor
store(new A(), 3000);
Object.defineProperty(A.prototype, "v", { set(x){ this.f = -x; }, get(){ return 0; }, configurable: true });
log(store(new A(), 5));
// setter removed from prototype: plain data store
const c = new A(); store(c, 3000);
delete A.prototype.v; log(store(c, 4), c.v, Object.keys(c).join());
// getter-only accessor defined later on the prototype, sloppy no-op and strict throw
Object.defineProperty(A.prototype, "v", { get(){ return 42; }, configurable: true });
log(store(c, 4) + ":" + c.v);
function strictStore(o){ "use strict"; try { for (let i = 0; i < 3; i++) o.v = i; } catch (e) { return e.constructor.name; } }
log(strictStore(new A()));
// accessor defined later, after the site cached a data-property store
class B {} const bs = [new B(), new B()];
function setX(o, n){ for (let i = 0; i < n; i++) o.x = i; return o.x; }
log(setX(bs[0], 3000));
Object.defineProperty(B.prototype, "x", { set(v){ this._x = v * 2; }, get(){ return this._x; }, configurable: true });
log(setX(bs[1], 5), Object.keys(bs[1]).join());
// Object.setPrototypeOf moves a receiver between a setter and no setter
const p1 = { set s(v){ this.a = "p1" + v; } }, p2 = { set s(v){ this.a = "p2" + v; } }, p3 = {};
const r = Object.create(p1);
function setS(o, n){ for (let i = 0; i < n; i++) o.s = i; return o.a; }
log(setS(r, 3000)); Object.setPrototypeOf(r, p2); log(setS(r, 5)); Object.setPrototypeOf(r, p3); log(setS(r, 5), Object.keys(r).join());
Object.setPrototypeOf(p3, p1); log(setS(r, 5));
// setter on prototype redefined to a different function on the same shape (pair slot)
const q = { set z(v){ this.w = 1; }, get z(){ return 0; } }; const qc = Object.create(q);
function setZ(o, n){ for (let i = 0; i < n; i++) o.z = i; return o.w; }
log(setZ(qc, 3000)); Object.defineProperty(q, "z", { set(v){ this.w = 2; }, get(){ return 0; } }); log(setZ(qc, 5));
// a proxy inserted up the chain mid-loop
const traps = [];
const prx = new Proxy({}, { set(t, k, v, rcv){ traps.push(k); return Reflect.set(t, k, v, rcv); } });
const base = { set y(v){ this.yy = v; } }; const leaf = Object.create(base);
function setY(o, n){ for (let i = 0; i < n; i++) o.y = i; return o.yy; }
log(setY(leaf, 3000)); Object.setPrototypeOf(base, prx); delete base.y; log(setY(leaf, 3), traps.join());
// frozen receiver
const fz = Object.create({ set k(v){ this.kk = v; } }); function setK(o, n){ for (let i = 0; i < n; i++) o.k = i; return o.kk; }
log(setK(fz, 3000)); Object.freeze(fz); log(setK(fz, 3));
// symbol-keyed setter
const sym = Symbol("s"); const so = { set [sym](v){ this.t = v; } }; function setSym(o, n){ for (let i = 0; i < n; i++) o[sym] = i; return o.t; } log(setSym(so, 3000));
print(out.join("|"));
"#;
    assert_eq!(printed(CACHED, "setter_cache.js"), ["3000|300|3000,9|-4|-2999,3,f,v|-2999:3|TypeError|2999|8,_x|p12999|p24|p24,a,s|p24|1|2|2999|2999,y|2999|2999|2999"]);
}
