// a.x read: one property read off a monomorphic object, N times.
const o = { x: 1 };
const N = 20000000;
let s = 0;
const t0 = performance.now();
for (let i = 0; i < N; i++) s += o.x;
const t1 = performance.now();
console.log("prop " + ((t1 - t0) * 1e6 / N).toFixed(3) + " ns/op " + s);
