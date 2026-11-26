// f(x): a small monomorphic call returning x+1, N times.
function f(x) { return (x + 1) | 0; }
const N = 20000000;
let s = 0;
const t0 = performance.now();
for (let i = 0; i < N; i++) s = f(s);
const t1 = performance.now();
console.log("call " + ((t1 - t0) * 1e6 / N).toFixed(3) + " ns/op " + s);
