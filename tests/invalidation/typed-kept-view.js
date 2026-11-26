// A typed array view an access proved is kept for the accesses after it (TOD-2417): a call that detaches the
// buffer, a register given another array, a store through a copy, and a base of another kind each retire it.
function sum(p, q, detach) {
  let s = p[0] + p[1];
  if (detach) structuredClone(p.buffer, { transfer: [p.buffer] });
  s += p[2] === undefined ? 1000 : p[2];
  const r = p; r[3] = 7; s += p[3] ?? 100;
  p = q; s += p[0];
  return s;
}
function walk(p, n) { let x = 0, i = p[0]; while (i >= 0 && n-- > 0) { x += i; i = p[i]; } p[0] = x; return x; }
let total = 0;
for (let k = 0; k < 3000; k++) {
  const p = new Int32Array([1, 2, 3, 0]), q = new Int32Array([5]);
  total += sum(p, q, false);
  const w = new Int32Array([2, -1, 1]);
  total += walk(w, 4);
}
const detached = sum(new Int32Array([1, 2, 3, 0]), new Int32Array([5]), true);
const other = sum(new Float64Array([0.5, 1, 2, 0]), new Uint8Array([300]), false);
let leftover = 0;
try { leftover = walk(new Float64Array([1.5, -1]), 2); } catch (e) { leftover = -1; }
const result = [total, detached, other, leftover].join();
if (result !== "63000,1108,54.5,1.5") throw new Error("typed kept view: " + result);
console.log("ok typed-kept-view");
