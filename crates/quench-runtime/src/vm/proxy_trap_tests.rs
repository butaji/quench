//! Stores to and reads of a proxy whose `get` or `set` trap the lane activates
//! (`proxy_traps::lane_read_get_trap`, `lane_store_set_trap`), against Node's output.
use crate::test_host::printed;

/// The reads `proxy_get_traps_match_node` runs.
const SOURCE: &str = r#"
const o = []; const log = [];
const t = { a: 1, b: 2, get g() { return this === p ? "recv" : "target"; } };
Object.defineProperty(t, "frozen", { value: 7, writable: false, configurable: false });
Object.defineProperty(t, "noget", { set(v) {}, configurable: false });
const h = { get(target, key, receiver) { log.push(key); return key === "frozen" ? 8 : key === "noget" ? 1 : Reflect.get(target, key, receiver); } };
const p = new Proxy(t, h);
function rd(n) { let s = 0; for (let i = 0; i < n; i++) s += p.a + p.b + (p.zz === undefined ? 1 : 0); return s; }
o.push(rd(3000), log.length, p.g);
for (const k of ["frozen", "noget"]) { try { o.push(p[k]); } catch (e) { o.push(e.constructor.name); } }
function fz(n) { try { for (let i = 0; i < n; i++) p.frozen; } catch (e) { return e.constructor.name; } return "none"; }
function ng(n) { try { for (let i = 0; i < n; i++) p.noget; } catch (e) { return e.constructor.name; } return "none"; }
o.push(fz(100), ng(100));
// trap this, arguments, receiver via prototype chain, method call through proxy, nested proxy
const h2 = { tag: "H", get(target, key, receiver) { return [this.tag, typeof key, receiver === child, arguments.length].join(); } };
const p2 = new Proxy({}, h2); const child = Object.create(p2);
function viaChild(n) { let r; for (let i = 0; i < n; i++) r = child.x; return r; }
function direct(n) { let r; for (let i = 0; i < n; i++) r = p2.x; return r; }
o.push(viaChild(500), direct(500));
const obj = { v: 5, m() { return this.v * 2; } }; const p3 = new Proxy(obj, { get(t, k, r) { return Reflect.get(t, k, r); } });
const p4 = new Proxy(p3, { get(t, k, r) { return Reflect.get(t, k, r); } });
function meth(n) { let s = 0; for (let i = 0; i < n; i++) s += p3.m() + p4.m() + p4.v; return s; }
o.push(meth(2000));
// handler changes mid-loop: trap replaced, removed, made a getter, non-callable; revoked proxy
const h5 = { get() { return 1; } }; const p5 = new Proxy({ k: 9 }, h5);
function chg(n) { const out = []; for (let i = 0; i < n; i++) { out.push(p5.k); if (i === 1) h5.get = () => 2; if (i === 2) delete h5.get; if (i === 3) Object.defineProperty(h5, "get", { get() { return () => 4; }, configurable: true }); if (i === 4) Object.defineProperty(h5, "get", { value: 5, configurable: true }); } return out; }
try { o.push(chg(6).join()); } catch (e) { o.push(e.constructor.name); }
const { proxy: pr, revoke } = Proxy.revocable({}, { get() { return 1; } });
function rev(n) { try { for (let i = 0; i < n; i++) { pr.x; if (i === 50) revoke(); } } catch (e) { return e.constructor.name; } }
o.push(rev(100));
// trap throws, trap returns via try/finally, array target, symbol key, typeof/in
const p6 = new Proxy({}, { get(t, k) { if (k === "boom") throw new RangeError("b"); try { return k; } finally { log.length = 0; } } });
function th(n) { try { for (let i = 0; i < n; i++) { p6.ok; if (i === 20) p6.boom; } } catch (e) { return e.message + log.length; } }
o.push(th(100));
const pa = new Proxy([1, 2, 3], { get(t, k, r) { return Reflect.get(t, k, r); } });
function arr(n) { let s = 0; for (let i = 0; i < n; i++) s += pa.length + pa[1]; return s; }
o.push(arr(500), pa.map(x => x * 2).join(), typeof p.a, "a" in p, Object.keys(p).join(), JSON.stringify(p3));
// async / generator trap callers, return directly
const pg = new Proxy({ a: 1 }, { get(t, k) { return t[k]; } });
function retd(i) { return pg.a; } let acc = 0; for (let i = 0; i < 2000; i++) acc += retd(i);
function* gen() { for (let i = 0; i < 3; i++) yield pg.a + i; }
o.push(acc, [...gen()].join());
(async () => { let s = 0; for (let i = 0; i < 1000; i++) { s += pg.a; if (i % 250 === 0) await null; } return s; })().then(v => { o.push(v); print(o.join("|")); });
"#;

/// A trapped read calls the trap on its handler with the target, the key and the receiver,
/// and yields what it returns; a target's frozen or getterless property still checks the
/// result; a trap replaced, removed, made an accessor or a non-function mid-loop, a revoked
/// proxy, a throwing trap, an array target, nested proxies, and generator and async callers
/// keep their semantics.
#[test]
fn proxy_get_traps_match_node() {
    assert_eq!(printed(SOURCE, "proxy-get.js"), ["12000|9000|recv|TypeError|TypeError|TypeError|TypeError|H,string,true,3|H,string,false,3|50000|TypeError|TypeError|b0|2500|2,4,6|number|true|a,b,g|{\"v\":5}|2000|1,2,3|1000"]);
}

/// The stores `proxy_set_traps_match_node` runs.
const SET_SOURCE: &str = r#"
const o = []; const log = [];
const p1 = new Proxy({ a: 1 }, { set(t, k, v, r) { log.push(typeof k + ":" + k + ":" + v + ":" + (r === p1)); t[k] = v; return true; } });
const p2 = new Proxy({ a: 1 }, { set() { return false; } });
const p3 = new Proxy({ a: 1 }, { set(t, k, v) { throw new RangeError("thrown " + v); } });
const p4 = new Proxy(Object.freeze({ a: 1 }), { set() { return true; } });
const p5 = new Proxy({ a: 1 }, { set() { return 0; } });
const p6 = new Proxy({}, { set() { return undefined; } });
function sloppy(p, n) { let last; for (let i = 0; i < n; i++) last = (p.a = i); return last; }
function strict(p, n) { "use strict"; let last; for (let i = 0; i < n; i++) last = (p.a = i); return last; }
function caught(p, n) { "use strict"; const r = []; for (let i = 0; i < n; i++) { try { p.zz = i; r.push("ok"); } catch (e) { r.push(e.constructor.name); } finally { r.push("f"); } } return r.join(); }
function sloppyRes(p, n) { const r = []; for (let i = 0; i < n; i++) r.push(p.zz = i); return r.join(); }
o.push(sloppy(p1, 5), strict(p1, 5), log.length, log.slice(0, 2).join(";"));
o.push(sloppy(p2, 5), caught(p2, 3), sloppyRes(p2, 3), caught(p3, 2), sloppyRes(p5, 3), caught(p5, 2), caught(p6, 2), caught(p4, 2));
const child = Object.create(p1);
function viaChild(n) { "use strict"; for (let i = 0; i < n; i++) child.x = i; return Object.keys(child).join() + ":" + p1.x; }
o.push(viaChild(5));
const h = { v: 7, set(t, k, val) { t[k] = this.v + val; return true; } };
const p8 = new Proxy({}, h);
function viaThis(n) { "use strict"; for (let i = 0; i < n; i++) p8.w = i; return p8.w; }
o.push(viaThis(5));
const p9 = new Proxy({}, { set(t, k, v) { Object.defineProperty(t, k, { value: 1, writable: false, configurable: true }); return true; } });
function nonWritable(n) { "use strict"; for (let i = 0; i < n; i++) p9.n = i; return p9.n; }
o.push(nonWritable(5));
const t10 = {}; Object.defineProperty(t10, "a", { get() { return 1; }, configurable: false });
const p10 = new Proxy(t10, { set() { return true; } });
function noSetter(n) { "use strict"; try { for (let i = 0; i < n; i++) p10.a = i; } catch (e) { return e.constructor.name; } }
o.push(noSetter(5));
const hc = { set() { return true; } }; const p11 = new Proxy({}, hc);
function swap(n) { "use strict"; const r = []; for (let i = 0; i < n; i++) { try { r.push(p11.q = i); } catch (e) { r.push(e.constructor.name); } if (i === 1) hc.set = () => false; if (i === 2) delete hc.set; if (i === 3) hc.set = 5; } return r.join(); }
o.push(swap(6));
function* gen() { for (let i = 0; i < 3; i++) { p1.g = i; yield p1.g; } }
o.push([...gen()].join());
(async () => { for (let i = 0; i < 100; i++) { p1.h = i; if (i % 50 === 0) await null; } return p1.h; })().then(v => { o.push(v); print(o.join("|")); });
"#;

/// A trapped store calls the trap on its handler with the target, the key, the value and the
/// receiver, and yields the value stored; a falsish result is a `TypeError` of a strict store
/// only (raised at the store, inside its `try`), a throwing trap, a trap replaced, removed or
/// made non-callable mid-loop, a frozen or getterless target, a proxy up the receiver's chain,
/// and generator and async callers keep their semantics.
#[test]
fn proxy_set_traps_match_node() {
    assert_eq!(printed(SET_SOURCE, "proxy-set.js"), ["4|4|10|string:a:0:true;string:a:1:true|4|TypeError,f,TypeError,f,TypeError,f|0,1,2|RangeError,f,RangeError,f|0,1,2|TypeError,f,TypeError,f|TypeError,f,TypeError,f|ok,f,ok,f|:4|11|1|TypeError|0,1,TypeError,3,TypeError,TypeError|0,1,2|99"]);
}

/// The stores `reflect_set_through_proxy_receivers_matches_node` runs.
const RECEIVER_SOURCE: &str = r#"
const o = [];
const log = [];
function show(x) { return typeof x === 'object' ? JSON.stringify(x) : String(x); }
// trapless receiver
const t1 = { a: 1 }; const p1 = new Proxy(t1, {});
o.push(Reflect.set(t1, 'a', 5, p1), t1.a, Reflect.set(t1, 'b', 6, p1), show(t1));
// receiver differs from target
const other = { a: 0 }; const p2 = new Proxy(other, {});
o.push(Reflect.set({ a: 1 }, 'a', 9, p2), show(other), Reflect.set({ x: 1 }, 'x', 9, p2), show(other));
// receiver with gopd / defineProperty traps
const p3 = new Proxy({ a: 1 }, { getOwnPropertyDescriptor(t, k) { log.push('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k); }, defineProperty(t, k, d) { log.push('def:' + k + ':' + JSON.stringify(d)); return Reflect.defineProperty(t, k, d); } });
o.push(Reflect.set({ a: 1 }, 'a', 7, p3), Reflect.set({}, 'n', 8, p3), log.join(';'));
// only one of them
const log2 = [];
const p4 = new Proxy({ a: 1 }, { defineProperty(t, k, d) { log2.push('def:' + k); return Reflect.defineProperty(t, k, d); } });
o.push(Reflect.set({}, 'a', 3, p4), log2.join(';'));
const p5 = new Proxy({ a: 1 }, { getOwnPropertyDescriptor(t, k) { log2.push('gopd:' + k); return Reflect.getOwnPropertyDescriptor(t, k); } });
o.push(Reflect.set({}, 'a', 4, p5), log2.join(';'));
// nested trapless proxies, non-writable, accessor, frozen, non-extensible receiver targets
const ro = {}; Object.defineProperty(ro, 'a', { value: 1, writable: false, configurable: true });
const acc = { get a() { return 1; } };
const fr = Object.freeze({ a: 1 }); const ne = Object.preventExtensions({ a: 1 });
const nest = new Proxy(new Proxy({ a: 1 }, {}), {});
o.push(Reflect.set({}, 'a', 2, new Proxy(ro, {})), Reflect.set({}, 'a', 2, new Proxy(acc, {})), Reflect.set({}, 'a', 2, new Proxy(fr, {})),
  Reflect.set({}, 'z', 2, new Proxy(ne, {})), Reflect.set({}, 'a', 2, new Proxy(ne, {})), Reflect.set({}, 'a', 2, nest), show(nest));
// array receiver, index and length
const arr = [1, 2]; const pa = new Proxy(arr, {});
o.push(Reflect.set([], 0, 'x', pa), Reflect.set([], 5, 'y', pa), Reflect.set([], 'length', 1, pa), show(arr));
// symbol key, revoked, getter on receiver chain
const sym = Symbol('s'); const ps = new Proxy({}, {});
o.push(Reflect.set({}, sym, 1, ps), ps[sym]);
const rv = Proxy.revocable({}, {}); rv.revoke();
try { o.push(Reflect.set({ a: 1 }, 'a', 1, rv.proxy)); } catch (e) { o.push(e.constructor.name); }
// handler trap added later / defined as getter / on handler prototype
const h = {}; const p6 = new Proxy({ a: 1 }, h);
const l3 = [];
function again() { return Reflect.set({ a: 0 }, 'a', 1, p6); }
o.push(again()); h.defineProperty = function (t, k, d) { l3.push('def'); return Reflect.defineProperty(t, k, d); }; o.push(again(), l3.join());
const hp = Object.create({ getOwnPropertyDescriptor(t, k) { l3.push('protogopd'); return Reflect.getOwnPropertyDescriptor(t, k); } });
const p7 = new Proxy({ a: 1 }, hp); o.push(Reflect.set({}, 'a', 1, p7), l3.join());
const hg = { get defineProperty() { l3.push('getter'); return undefined; } };
const p8 = new Proxy({ a: 1 }, hg); o.push(Reflect.set({}, 'a', 1, p8), l3.join());
// set trap calling Reflect.set with receiver (Vue pattern) in a loop, stack depth
const t9 = { n: 0 }; const p9 = new Proxy(t9, { set(t, k, v, r) { const old = t[k]; const ok = Reflect.set(t, k, v, r); if (ok && old !== v) log.push('trig'); return ok; }, get(t, k, r) { return Reflect.get(t, k, r); } });
function loop(n) { 'use strict'; for (let i = 0; i < n; i++) p9.n = i; return p9.n; }
o.push(loop(2000), log.filter(x => x === 'trig').length);
const sub = Object.create(p9);
function loop2(n) { 'use strict'; for (let i = 0; i < n; i++) sub.n = i; return sub.n + ':' + Object.keys(sub).join(); }
o.push(loop2(10), t9.n);
print(o.join('|'));
"#;

/// `Reflect.set` with a proxy receiver defines through the receiver's `getOwnPropertyDescriptor`
/// and `defineProperty` traps wherever its handler has them, in plain, inherited and
/// accessor-defined places, and forwards to the target where it has neither (nested proxies,
/// read-only, accessor, frozen and non-extensible targets, indices, `length`, symbols and a
/// revoked proxy included), also from a `set` trap running in the lane.
#[test]
fn reflect_set_through_proxy_receivers_matches_node() {
    assert_eq!(printed(RECEIVER_SOURCE, "reflect-set-receiver.js"), ["true|5|true|{\"a\":5,\"b\":6}|true|{\"a\":9}|true|{\"a\":9,\"x\":9}|true|true|gopd:a;def:a:{\"value\":7};gopd:n;def:n:{\"value\":8,\"writable\":true,\"enumerable\":true,\"configurable\":true}|true|def:a|true|def:a;gopd:a|false|false|false|false|true|true|{\"a\":2}|true|true|true|[\"x\"]|true|1|TypeError|true|true|def|true|def,protogopd|true|def,protogopd,getter|1999|1999|9:n|1999"]);
}

/// The source `trapless_proxy_reads_match_node` runs.
const TRAPLESS_GET_SOURCE: &str = r#"
const o = [];
const base = { inherited: 'inh', get g() { return this === p1 ? 'proxy' : this === t1 ? 'target' : 'other'; } };
const t1 = Object.create(base); t1.a = 1; t1.s = 'str'; t1[Symbol.for('k')] = 'sym';
Object.defineProperty(t1, 'ro', { value: 5, writable: false, configurable: false });
const p1 = new Proxy(t1, {});
function reads(p, n) { let r = ''; for (let i = 0; i < n; i++) { r = [p.a, p.s, p.inherited, p.missing, p.g, p.ro, p.length].join(); } return r; }
o.push(reads(p1, 5));
const pp = new Proxy(new Proxy(t1, {}), {});
o.push(reads(pp, 5));
function ret(p) { return p.a; }
function ret2(p) { return p.g; }
let acc = 0; for (let i = 0; i < 100; i++) acc += ret(p1) + (ret2(p1) === 'proxy' ? 1 : 0);
o.push(acc);
// array and function and class targets
const pa = new Proxy([1, 2, 3], {});
function arr(n) { let s = 0; for (let i = 0; i < n; i++) s += pa[1] + pa.length; return s; }
o.push(arr(10));
const pf = new Proxy(function f(a, b) {}, {});
function fn(n) { let s = ''; for (let i = 0; i < n; i++) s = pf.name + pf.length + typeof pf.call; return s; }
o.push(fn(5));
// handler changes: trap added after reads, trap removed, trap on handler's prototype, getter trap
const h = {}; const p2 = new Proxy({ a: 1 }, h);
function chg(n) { const out = []; for (let i = 0; i < n; i++) { out.push(p2.a); if (i === 1) h.get = () => 'trap'; if (i === 2) delete h.get; if (i === 3) Object.setPrototypeOf(h, { get: () => 'proto' }); if (i === 4) Object.setPrototypeOf(h, null); } return out.join(); }
o.push(chg(7));
// revoked
const rv = Proxy.revocable({ a: 1 }, {});
function rev(n) { let r = ''; try { for (let i = 0; i < n; i++) { r += rv.proxy.a; if (i === 2) rv.revoke(); } } catch (e) { r += e.constructor.name; } return r; }
o.push(rev(6));
// getter throws, setter-only accessor, non-callable proxies, symbols
const t3 = { get boom() { throw new RangeError('b'); }, set only(v) {} };
const p3 = new Proxy(t3, {});
function thr(n) { try { for (let i = 0; i < n; i++) p3.boom; } catch (e) { return e.constructor.name; } }
function setonly(n) { let s; for (let i = 0; i < n; i++) s = p3.only; return String(s); }
o.push(thr(4), setonly(4));
// typeof / method calls through the proxy: this is the proxy
const t4 = { v: 3, m() { return this === p4; }, n() { return this.v; } };
const p4 = new Proxy(t4, {});
function meth(n) { let r; for (let i = 0; i < n; i++) r = [p4.m(), p4.n()].join(); return r; }
o.push(meth(5));
// generator and async readers
function* gen() { for (let i = 0; i < 3; i++) yield p1.a + i; }
o.push([...gen()].join());
// prototype chain: object inheriting from a trapless proxy; receiver is the child
const child = Object.create(p1);
function viaChild(n) { let r; for (let i = 0; i < n; i++) r = child.g + child.a; return r; }
o.push(viaChild(5));
// stack depth and deep nesting
let deep = { v: 1 }; for (let i = 0; i < 50; i++) deep = new Proxy(deep, {});
function dp(n) { let s = 0; for (let i = 0; i < n; i++) s += deep.v; return s; }
o.push(dp(5));
print(o.join('|'));
"#;

/// A read through proxies whose handlers have no `get` trap completes in the lane (a data value, an absent key, a getter run with the proxy as receiver) as the target's would, through nested proxies, arrays, functions, revoked proxies, handlers gaining or losing the trap, prototype chains and returning reads.
#[test]
fn trapless_proxy_reads_match_node() {
    assert_eq!(printed(TRAPLESS_GET_SOURCE, "get2.js"), ["1,str,inh,,proxy,5,|1,str,inh,,other,5,|200|50|f2function|1,1,trap,1,proto,1,1|111TypeError|RangeError|undefined|true,3|1,2,3|other1|5"]);
}

/// The source `trapless_proxy_stores_match_node` runs.
const TRAPLESS_SET_SOURCE: &str = r#"
const o = [];
const log = [];
const base = { set inh(v) { log.push('setter:' + (this === p1) + ':' + v); }, ro: 1 };
Object.defineProperty(base, 'ro2', { value: 1, writable: false, configurable: true });
const t1 = Object.create(base); t1.a = 1;
Object.defineProperty(t1, 'ro', { value: 5, writable: false, configurable: true });
const p1 = new Proxy(t1, {});
function w(p, n) { 'use strict'; let last; for (let i = 0; i < n; i++) { last = (p.a = i); p.fresh = i; } return last; }
o.push(w(p1, 5), JSON.stringify(t1), Object.keys(t1).join());
function setter(p, n) { 'use strict'; for (let i = 0; i < n; i++) p.inh = i; return log.length + ':' + log[0]; }
o.push(setter(p1, 3));
function ro(p, n) { 'use strict'; const r = []; for (let i = 0; i < n; i++) { try { p.ro = i; r.push('w'); } catch (e) { r.push(e.constructor.name); } try { p.ro2 = i; r.push('w'); } catch (e) { r.push(e.constructor.name); } } return r.join(); }
o.push(ro(p1, 2));
function sloppyRo(p, n) { let r = 0; for (let i = 0; i < n; i++) { p.ro = i; r += (p.ro === 5 ? 1 : 0); } return r; }
o.push(sloppyRo(p1, 3));
// nested trapless; frozen / sealed / non-extensible
const nest = new Proxy(new Proxy({ x: 0 }, {}), {});
function nn(n) { 'use strict'; for (let i = 0; i < n; i++) nest.x = i; return nest.x; }
o.push(nn(5));
const fz = new Proxy(Object.freeze({ x: 1 }), {}); const ne = new Proxy(Object.preventExtensions({ x: 1 }), {}); const se = new Proxy(Object.seal({ x: 1 }), {});
function fr(p, n) { 'use strict'; const r = []; for (let i = 0; i < n; i++) { try { p.x = i + 10; r.push('x'); } catch (e) { r.push('E'); } try { p.y = i; r.push('y'); } catch (e) { r.push('E'); } } return r.join(''); }
o.push(fr(fz, 2), fr(ne, 2), fr(se, 2));
// handler has defineProperty / gopd traps, set trap undefined, handlers changing
const dlog = [];
const pd = new Proxy({ a: 1 }, { defineProperty(t, k, d) { dlog.push('d:' + k); return Reflect.defineProperty(t, k, d); } });
const pg = new Proxy({ a: 1 }, { getOwnPropertyDescriptor(t, k) { dlog.push('g:' + k); return Reflect.getOwnPropertyDescriptor(t, k); } });
function viaTraps(p, n) { 'use strict'; for (let i = 0; i < n; i++) p.a = i; return p.a; }
o.push(viaTraps(pd, 3), viaTraps(pg, 3), dlog.join());
const h = {}; const ph = new Proxy({ a: 0 }, h);
function chg(n) { 'use strict'; const r = []; for (let i = 0; i < n; i++) { ph.a = i; r.push(ph.a); if (i === 1) h.set = (t, k, v) => { t[k] = v * 100; return true; }; if (i === 2) delete h.set; if (i === 3) h.set = undefined; if (i === 4) h.defineProperty = (t, k, d) => Reflect.defineProperty(t, k, { ...d, value: -1 }); } return r.join(); }
o.push(chg(7));
// receiver is a child of the proxy; array, function targets; symbols; numeric keys
const child = Object.create(p1);
function viaChild(n) { 'use strict'; for (let i = 0; i < n; i++) child.a = i; return Object.keys(child).join() + ':' + t1.a; }
o.push(viaChild(4));
const pa = new Proxy([1, 2], {});
function arr(n) { 'use strict'; for (let i = 0; i < n; i++) { pa[i] = i * 2; pa.length = n; } return JSON.stringify(pa); }
o.push(arr(4));
const pfn = new Proxy(function () {}, {});
function fn(n) { 'use strict'; for (let i = 0; i < n; i++) pfn.tag = i; return pfn.tag; }
o.push(fn(3));
const rv = Proxy.revocable({}, {});
function rev(n) { 'use strict'; let r = ''; try { for (let i = 0; i < n; i++) { rv.proxy.a = i; r += i; if (i === 2) rv.revoke(); } } catch (e) { r += e.constructor.name; } return r; }
o.push(rev(6));
// setting a function value invalidates method caches
const t5 = { m() { return 1; } }; const p5 = new Proxy(t5, {});
function meth(n) { 'use strict'; let s = 0; for (let i = 0; i < n; i++) { s += t5.m(); if (i === 2) p5.m = () => 10; } return s; }
o.push(meth(6));
print(o.join('|'));
"#;

/// A store through proxies whose handlers have no `set`, `getOwnPropertyDescriptor` or `defineProperty` trap writes the target in the lane as OrdinarySet with the proxy as receiver does: setters, read-only and frozen properties, nested proxies, inherited receivers, handlers gaining traps, and revoked proxies keep their semantics.
#[test]
fn trapless_proxy_stores_match_node() {
    assert_eq!(printed(TRAPLESS_SET_SOURCE, "set2.js"), ["4|{\"a\":4,\"fresh\":4}|a,fresh|3:setter:true:0|TypeError,TypeError,TypeError,TypeError|3|4|EEEE|xExE|xExE|2|2|d:a,d:a,d:a,g:a,g:a,g:a|0,1,200,3,4,-1,-1|a:4|[0,2,4,6]|2|012TypeError|33"]);
}

/// The source `set_trap_results_through_returned_calls_match_node` runs.
const CHAINED_SET_SOURCE: &str = r#"
const o = [];
function no() { return false; }
function yes(t, k, v) { t[k] = v; return true; }
function deeper(x) { return no(x); }
const pChainFalse = new Proxy({}, { set(t, k, v, r) { return no(); } });
const pChainTrue = new Proxy({}, { set(t, k, v, r) { return yes(t, k, v); } });
const pChain2 = new Proxy({}, { set(t, k, v, r) { return deeper(v); } });
const pNative = new Proxy({}, { set(t, k, v, r) { return Reflect.set(t, k, v, r); } });
const pNativeFalse = new Proxy(Object.freeze({ z: 1 }), { set(t, k, v, r) { return Reflect.set(t, k, v, r); } });
const pTernary = new Proxy({}, { set(t, k, v) { return v % 2 === 0 ? yes(t, k, v) : no(); } });
function run(p, n) { 'use strict'; const r = []; for (let i = 0; i < n; i++) { try { p.q = i; r.push('ok'); } catch (e) { r.push(e.constructor.name + ':' + e.message.replace(/ for property.*/, '')); } } return r.join(); }
function sloppy(p, n) { const r = []; for (let i = 0; i < n; i++) r.push(p.q = i); return r.join(); }
for (const p of [pChainFalse, pChainTrue, pChain2, pNative, pTernary]) o.push(run(p, 4), sloppy(p, 3));
function nf(n) { 'use strict'; const r = []; for (let i = 0; i < n; i++) { try { pNativeFalse.z = i; r.push('ok'); } catch (e) { r.push(e.constructor.name); } } return r.join(); }
o.push(nf(3));
// property name in the message
const pm = new Proxy({}, { set() { return false; } });
function msg(n) { 'use strict'; let m; for (let i = 0; i < n; i++) { try { pm.someName = i; } catch (e) { m = e.message; } } return m; }
o.push(msg(3));
function thisField() { 'use strict'; const r = []; for (let i = 0; i < 3; i++) { try { pm.x = 1; } catch (e) { r.push('E'); } } return r.join(); }
o.push(thisField());
print(o.join('|'));
"#;

/// A strict store's falsish result is the `TypeError` of the store wherever the trap returns it from: its own `return`, a returned call to a function or a native (`Reflect.set`), and a chain of returned calls.
#[test]
fn set_trap_results_through_returned_calls_match_node() {
    assert_eq!(printed(CHAINED_SET_SOURCE, "set3.js"), ["TypeError:'set' on proxy: trap returned falsish,TypeError:'set' on proxy: trap returned falsish,TypeError:'set' on proxy: trap returned falsish,TypeError:'set' on proxy: trap returned falsish|0,1,2|ok,ok,ok,ok|0,1,2|TypeError:'set' on proxy: trap returned falsish,TypeError:'set' on proxy: trap returned falsish,TypeError:'set' on proxy: trap returned falsish,TypeError:'set' on proxy: trap returned falsish|0,1,2|ok,ok,ok,ok|0,1,2|ok,TypeError:'set' on proxy: trap returned falsish,ok,TypeError:'set' on proxy: trap returned falsish|0,1,2|TypeError,TypeError,TypeError|'set' on proxy: trap returned falsish for property 'someName'|E,E,E"]);
}
