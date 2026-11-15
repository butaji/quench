// Differential scenarios for proved int regions (`vm::dispatch_fast::region_state`): the specialized lane and the general path print the same lines.
const out = [];
const log = (name, v) => out.push(name + " " + (typeof v === "string" ? v : Object.is(v, -0) ? "-0" : String(v)));

function intloop(n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + i * 3) | 0; } return s; }
function intloopSub(n) { let s = 7; for (let i = 0; i < n; i++) { s = (s - i * 5) | 0; } return s; }
function intloopSwap(n) { let s = 7; for (let i = 0; i < n; i++) { s = (i * 5 - s) | 0; } return s; }
function intloopNeg(n) { let s = 1; for (let i = 0; i < n; i++) { s = (s + i * -2097151) | 0; } return s; }
function intloopBig(n) { let s = 1; for (let i = 0; i < n; i++) { s = (s + i * 2097152) | 0; } return s; }
function intloopBigger(n) { let s = 1; for (let i = 0; i < n; i++) { s = (s + i * 4194304) | 0; } return s; }
function sumloop(n, a) { let s = 0; for (let i = 0; i < n; i++) { s = (s + a) | 0; } return s; }
function downloop(n) { let s = 0; for (let i = n; i > 0; i--) { s = (s + i * 7) | 0; } return s; }
function downloop2(n, m) { let s = 0; for (let i = n; i > m; i--) { s = (s - i * 3) | 0; } return s; }
function leloop(n) { let s = 0; for (let i = 0; i <= n; i++) { s = (s + i * 3) | 0; } return s; }

for (const n of [0, 1, 2, 3, 10, 1000, -5]) {
  log("intloop " + n, intloop(n));
  log("intloopSub " + n, intloopSub(n));
  log("intloopSwap " + n, intloopSwap(n));
  log("intloopNeg " + n, intloopNeg(n));
  log("intloopBig " + n, intloopBig(n));
  log("intloopBigger " + n, intloopBigger(n));
  log("down " + n, downloop(n));
  log("down2 " + n, downloop2(n, 2));
  log("le " + n, leloop(n));
}
// warm so that regions prove, then perturb entry types
for (let k = 0; k < 50; k++) intloop(20);
// non-int entries
log("double n", intloop(10.5));
log("string n", intloop("12"));
log("obj valueOf n", intloop({ valueOf() { return 5; } }));
log("undefined n", intloop(undefined));
log("null n", intloop(null));
log("true n", intloop(true));
log("NaN n", intloop(NaN));
for (let k = 0; k < 50; k++) sumloop(10, 3);
log("sum double", sumloop(10, 0.5));
log("sum string", sumloop(3, "1"));
log("sum big", sumloop(10, 2147483647));
log("sum min", sumloop(10, -2147483648));
log("sum obj", sumloop(4, { valueOf() { return 9; } }));
log("sum undefined", sumloop(4, undefined));
log("sum bigint", (() => { try { return sumloop(4, 1n); } catch (e) { return e.constructor.name; } })());
// n near INT_MAX: only the first iterations run (bounded by the body throwing)
function nearMax(n, stop) { let c = 0; for (let i = n - 5; i < n; i++) { c = (c + 1) | 0; } return c; }
log("nearMax", nearMax(2147483647, 0));
log("nearMax2", nearMax(2147483646, 0));
function nearMin(n) { let c = 0; for (let i = n + 5; i > n; i--) { c = (c + 1) | 0; } return c; }
log("nearMin", nearMin(-2147483648));
log("nearMin2", nearMin(-2147483647));
function maxCounter(start, n) { let s = 0; for (let i = start; i < n; i++) { s = (s + i * 3) | 0; } return s; }
log("maxCounter", maxCounter(2147483640, 2147483647));
log("maxCounter lt", maxCounter(2147483640, 2147483646));
// counter modified in the body
function modCounter(n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + i * 3) | 0; i = i + 1; } return s; }
log("modCounter", modCounter(10));
function modLimit(n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + i * 3) | 0; n = n - 1; } return s; }
log("modLimit", modLimit(10));
function skipCounter(n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + i * 3) | 0; if (s > 100) i += 3; } return s; }
log("skipCounter", skipCounter(50));
// int overflow of the sum domain (truncation island)
function wrap(n) { let s = 2147483000; for (let i = 0; i < n; i++) { s = (s + i * 1000003) | 0; } return s; }
log("wrap", wrap(5000));
log("wrap2", wrap(50000));
// -0 products
function negz(n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + i * -3) | 0; } return s; }
log("negz", negz(3));
// exceptions thrown inside the loop
function thrower(n, at) { let s = 0; try { for (let i = 0; i < n; i++) { s = (s + i * 3) | 0; if (i === at) throw new Error("x" + s); } } catch (e) { return e.message; } return s; }
log("thrower", thrower(100, 50));
log("thrower none", thrower(100, 500));
// exits and re-entry: nested loops with changing types between entries
function nested(m, n) {
  let total = 0;
  for (let j = 0; j < m; j++) {
    let s = j;
    if (j === 3) s = 1.5;
    if (j === 5) s = "7";
    if (j === 6) s = { valueOf() { return 4; } };
    if (j === 7) s = 2147483647;
    for (let i = 0; i < n; i++) { s = (s + i * 3) | 0; }
    total = (total + s) | 0;
  }
  return total;
}
log("nested", nested(10, 7));
log("nested again", nested(10, 7));
log("nested 20", nested(20, 3));
// the same function alternating int and double entries many times (flip-flop retirement)
function flip(a, n) { let s = a; for (let i = 0; i < n; i++) { s = (s + i * 3) | 0; } return s; }
let acc = [];
for (let k = 0; k < 40; k++) acc.push(flip(k % 2 ? 1.5 : 1, 5));
log("flip", acc.join(","));
// registers dead at the header written in the body
function fresh(n) { let r = 0; for (let i = 0; i < n; i++) { const t = (i + 3) | 0; r = (r + t) | 0; } return r; }
log("fresh", fresh(100));
// generator and closure cases keep working
function* gen(n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + i * 3) | 0; yield s; } }
log("gen", [...gen(5)].join(","));
const closures = []; function cl(n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + i * 3) | 0; closures.push(() => s); } return s; }
log("closure", cl(5) + " " + closures.map(f => f()).join(","));
// large trip count
log("big", intloop(1000000));
log("bigdown", downloop(1000000));
for (const line of out) print(line);
