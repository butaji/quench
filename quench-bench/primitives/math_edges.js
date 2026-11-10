// Differential check: Math functions on edge values must print the same under every tier.
// Run under quench-node (default, QUENCH_DEV=js-lane-methods, --no-jit) and compare outputs.
"use strict";
const V = [0, -0, 0.5, -0.5, 1.5, -1.5, 2.5, 0.49999999999999994, -0.49999999999999994, 4503599627370495.5, 2 ** 52, 2 ** 53, -(2 ** 53),
  2147483647, 2147483648, -2147483648, -2147483649, 4294967296, 1e300, -1e300, Infinity, -Infinity, NaN, 1 / 3, -1 / 3, 7, -7, 3.7, -3.7, 65536 * 65536 + 5];
const fns = {
  floor: (x) => Math.floor(x), ceil: (x) => Math.ceil(x), trunc: (x) => Math.trunc(x), abs: (x) => Math.abs(x),
  sqrt: (x) => Math.sqrt(x), round: (x) => Math.round(x), fround: (x) => Math.fround(x),
  max: (x, y) => Math.max(x, y), min: (x, y) => Math.min(x, y), imul: (x, y) => Math.imul(x, y),
};
const out = [];
const show = (v) => (Object.is(v, -0) ? "-0" : String(v));
for (const [name, f] of Object.entries(fns)) {
  // Loop in one function so the hot loop is compiled, then compare per-value results.
  const run = new Function("f", "V", "r", `for (let k = 0; k < 400; k++) for (let i = 0; i < V.length; i++) for (let j = 0; j < (f.length > 1 ? V.length : 1); j++) { const v = f(V[i], V[j]); if (k === 399) r.push(String(v === 0 && 1 / v < 0 ? "-0" : v)); }`);
  const r = []; run(f, V, r); out.push(name + ":" + r.join(","));
}
console.log(out.join("\n"));
