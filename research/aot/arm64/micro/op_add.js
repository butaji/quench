// a + b: int32 add of two loop values, N times.
const N = 20000000;
let s = 0;
const t0 = performance.now();
for (let i = 0; i < N; i++) s = (s + i) | 0;
const t1 = performance.now();
console.log("add " + ((t1 - t0) * 1e6 / N).toFixed(3) + " ns/op " + s);
