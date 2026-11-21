// a[i]: element read from a packed int array, N times.
const a = [1, 2, 3, 4];
const N = 20000000;
let s = 0;
const t0 = performance.now();
for (let i = 0; i < N; i++) s += a[i & 3];
const t1 = performance.now();
console.log("index " + ((t1 - t0) * 1e6 / N).toFixed(3) + " ns/op " + s);
