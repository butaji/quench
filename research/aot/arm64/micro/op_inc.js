// i+1: int32 increment, N times.
const N = 20000000;
let a = 0;
const t0 = performance.now();
for (let i = 0; i < N; i++) a = (a + 1) | 0;
const t1 = performance.now();
console.log("inc " + ((t1 - t0) * 1e6 / N).toFixed(3) + " ns/op " + a);
