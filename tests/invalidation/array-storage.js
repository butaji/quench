// Element-kind proofs of arrays: int to double to object, holes, and length changes.
function sum(a) { let s = 0; for (let i = 0; i < a.length; i++) s += a[i] === undefined ? 1000 : a[i]; return s; }
const a = []; for (let i = 0; i < 100; i++) a.push(i);
let s = 0;
for (let k = 0; k < 300; k++) s = sum(a);
a[3] = 0.5;
const d = sum(a);
a[4] = { valueOf() { return 4; } };
const o = sum(a);
delete a[5];
const h = sum(a);
a.length = 10;
const l = sum(a);
if (s !== 4950 || d !== 4947.5 || typeof o !== "string" && o !== 4947.5 || l !== sum(a)) throw new Error("array storage: " + [s, d, o, h, l]);
console.log("ok array-storage");
