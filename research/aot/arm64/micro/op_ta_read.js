// u32[i&3]: element read from a Uint32Array, N times.
const a = new Uint32Array([1, 2, 3, 4]);
const N = 20000000;
let s = 0;
const t0 = performance.now();
for (let i = 0; i < N; i++) s += a[i & 3];
const t1 = performance.now();
console.log("ta_read " + ((t1 - t0) * 1e6 / N).toFixed(3) + " ns/op " + s);
