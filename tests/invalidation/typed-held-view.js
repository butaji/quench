// A typed view proved before a call stays proved past it while the code holds views (TOD-2417): a call
// that changes a live view (detach, resize, a write that unshares copied bytes) retires the code instead.
function noop(x) { return x + 1; }
function sum(a, n, change) {
  let s = 0;
  for (let i = 0; i < n; i++) { s += a[i & 7] ?? 1000; s = noop(s); if (i === 900) change(a); s += a[i & 7] ?? 1000; }
  return s;
}
const keep = () => {};
const detach = (a) => structuredClone(a.buffer, { transfer: [a.buffer] });
const shrink = (a) => a.buffer.resize(8);
let total = 0;
for (let k = 0; k < 200; k++) total += sum(new Int32Array([1, 2, 3, 4, 5, 6, 7, 8]), 1000, keep);
const detached = sum(new Int32Array([1, 2, 3, 4, 5, 6, 7, 8]), 1000, detach);
const shrunk = sum(new Int32Array(new ArrayBuffer(32, { maxByteLength: 64 })).fill(3), 1000, shrink);
// Straight-line code past a call no inlining takes (eight callees): the view proved before it is held after it.
const callees = []; for (let j = 0; j < 8; j++) callees.push(new Function("a", "s", "return s + " + j + ";"));
function around(a, f) { let s = a[0] + a[1]; s = f(a, s); return s + (a[2] ?? 1000) + (a[3] ?? 1000); }
let straight = 0;
for (let k = 0; k < 20000; k++) straight += around(new Int32Array([1, 2, 3, 4]), callees[k & 7]);
straight += around(new Int32Array([1, 2, 3, 4]), (a, s) => { detach(a); return s; });
straight += around(new Int32Array(new ArrayBuffer(16, { maxByteLength: 16 })).fill(5), (a, s) => { shrink(a); return s; });
const result = [total, detached, shrunk, straight].join();
if (result !== "2000000,208089,157547,274013") throw new Error("typed held view: " + result);
console.log("ok typed-held-view");
