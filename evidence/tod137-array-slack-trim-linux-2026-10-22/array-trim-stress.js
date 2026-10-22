// Arrays that grow by push, survive collections, and are then mutated while allocation
// triggers further collections. Prints a checksum of every observable result.
"use strict";
let checksum = 0;
const mix = (v) => { checksum = (Math.imul(checksum ^ (v | 0), 16777619) + 1) | 0; };
const keep = [];
function garbage(n) { let g = []; for (let i = 0; i < n; i++) g.push({ i, s: "x" + i }); return g.length; }
for (let round = 0; round < 60; round++) {
  const a = [];
  for (let i = 0; i < 37 + round; i++) a.push(i * 3);           // capacity > length
  keep.push(a);
  const shared = a.slice();                                      // copy
  const lit = [1, 2, 3]; keep.push(lit);                         // literal backing
  garbage(3000);                                                 // collections happen here
  a.push(round); a.pop(); a.push(round, round + 1);
  a.splice(5, 3, "s", "t"); a.unshift(-1); a.shift();
  a.sort((x, y) => { garbage(2); return String(x) < String(y) ? -1 : String(x) > String(y) ? 1 : 0; });
  mix(a.length); for (const v of a) mix(typeof v === "number" ? v : v.charCodeAt(0));
  mix(shared.length); mix(shared[shared.length - 1]);
  lit.push(4); mix(lit.reduce((s, v) => s + v, 0));
  a.length = 3; garbage(500); a.push(7); mix(a.join("").length);
  const e = []; e.push(1); e.pop(); garbage(200); e.push(9); mix(e[0]);
}
for (const arr of keep) { arr.reverse(); mix(arr[0] === undefined ? -7 : String(arr[0]).length); mix(arr.length); }
const big = []; for (let i = 0; i < 5000; i++) big.push(i); big.length = 10; garbage(20000);
for (let i = 0; i < 1000; i++) big.push(i); mix(big.length); mix(big[9]); mix(big[1009]);
console.log(checksum, keep.length);
