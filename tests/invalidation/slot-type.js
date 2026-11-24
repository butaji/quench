// An object field whose observed type the code relied on is widened by a later write.
function make(v) { return { a: 1, v: v }; }
function sum(objs) { let s = 0; for (let i = 0; i < objs.length; i++) s += objs[i].v; return s; }
const objs = []; for (let i = 0; i < 100; i++) objs.push(make(i));
let s = 0;
for (let k = 0; k < 300; k++) s = sum(objs);
objs[50].v = 0.5;
const d = sum(objs);
objs[60].v = "x";
const t = sum(objs);
if (s !== 4950 || d !== 4950 - 50 + 0.5 || typeof t !== "string") throw new Error("slot type: " + [s, d, t]);
console.log("ok slot-type");
