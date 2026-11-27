// Array-kind differentials: a loop whose array register alternates kinds, and one that
// writes through a copy of the array.
const u = new Uint32Array([1, 2, 3, 4]);
const f = new Float32Array([1.5, 2.5, 3.5, 4.5]);
const g = new Int16Array([-1, -2, -3, -4]);
let s = 0;
for (let i = 0; i < 20000; i++) { const a = (i & 1) ? u : f; s += a[i & 3]; }
let t = 0;
for (let i = 0; i < 20000; i++) { const a = (i % 3 === 0) ? u : (i % 3 === 1) ? f : g; t += a[(i >> 2) & 3]; }
let w = 0;
const out = new Float32Array(4);
for (let i = 0; i < 20000; i++) { const c = out; c[i & 3] = (i % 7) * 0.5; w += c[(i + 1) & 3]; }
if (typeof print === "function") print([s, t, w.toFixed(3)].join(",")); else console.log([s, t, w.toFixed(3)].join(","));
