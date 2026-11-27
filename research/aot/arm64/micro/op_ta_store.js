// f32[i&3] = v: element store to a Float32Array, N times.
const a = new Float32Array(4);
const N = 20000000;
let v = 0;
const t0 = performance.now();
for (let i = 0; i < N; i++) { a[i & 3] = i * 0.5; v += a[i & 3]; }
const t1 = performance.now();
console.log("ta_store " + ((t1 - t0) * 1e6 / N).toFixed(3) + " ns/op " + v.toFixed(0));
