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
