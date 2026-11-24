// A shape held past a call: the callee changes the object's shape, the held reads must see it.
function touch(o, k) { if (k === 4999) { delete o.a; o.b = 7; } }
function read(o, k) { const before = o.a; touch(o, k); return before + (o.a === undefined ? o.b * 100 : o.a); }
let total = 0;
for (let k = 0; k < 5000; k++) { const o = { a: 1, b: 2 }; total += read(o, k); }
if (total !== 4999 * 2 + 701) throw new Error("shape departure: " + total);
console.log("ok shape-departure");
