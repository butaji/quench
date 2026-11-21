"use strict";
// TOD-1807: a lane run continues past a native `gc()` call. What it resolved before the
// collection (call targets, capture storages) must not outlive it: a swept closure's heap
// index goes to the next closure made, which a stale target entered with the dead closure's
// captures (`k` read a Symbol).
const ops = {};
function addOne(x) { return (x + 1) | 0; }
ops.field_store = function (n) { const o = { x: 0, y: 0 }; for (let i = 0; i < n; i++) { o.y = i; } return o.y; };
ops.call_direct = function (n) { let a = 0; for (let i = 0; i < n; i++) { a = (a + addOne(i)) | 0; } return a; };
ops.call_closure = function (n) { const k = 5, f = (x) => (x + k) | 0; let a = 0; for (let i = 0; i < n; i++) { a = (a + f(i)) | 0; } return a; };
let sink = 0; const n = 20000;
for (const name of ["field_store", "call_direct", "call_closure"]) {
  const op = ops[name];
  for (let r = 0; r < 3; r++) sink ^= +op(n) | 0;
  gc(); sink ^= +op(n) | 0; sink ^= +op(n) | 0;
  gc(); sink ^= +op(n) | 0; sink ^= +op(2 * n) | 0;
}
console.log(sink);

// The running activation keeps storing fresh objects into a binding its closures capture
// while `gc()` runs in the same lane run.
function held(n) {
  let last = null, sum = 0;
  const put = (i) => { last = { i, tag: "v" + i }; };
  const get = () => last.i + last.tag.length;
  for (let i = 0; i < n; i++) {
    put(i);
    if (i % 4096 === 0) gc();
    sum = (sum + get()) | 0;
  }
  return sum;
}
let total = 0;
for (let r = 0; r < 4; r++) total = (total + held(30000)) | 0;
console.log(total);
