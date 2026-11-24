// The prototype link itself changes (setPrototypeOf), and a shadowing own property appears.
const base = { v: 1 };
const o = Object.create(base);
function read(x) { return x.v; }
let s = 0;
for (let k = 0; k < 20000; k++) s += read(o);
Object.setPrototypeOf(o, { v: 5 });
const a = read(o);
o.v = 9;
const b = read(o);
delete o.v;
base.v = 3;
const c = read(o);
if (s !== 20000 || a !== 5 || b !== 9 || c !== 5) throw new Error("prototype chain: " + [s, a, b, c]);
console.log("ok prototype-chain");
