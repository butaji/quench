// A getter's own-field return read inline; replacing the getter or adding a setter.
class T { constructor() { this._v = 4; } get v() { return this._v; } }
const t = new T();
function read(o) { return o.v; }
let s = 0;
for (let k = 0; k < 20000; k++) s += read(t);
Object.defineProperty(T.prototype, "v", { get() { return 40; }, configurable: true });
const a = read(t);
Object.defineProperty(t, "v", { value: 400 });
const b = read(t);
if (s !== 80000 || a !== 40 || b !== 400) throw new Error("accessor: " + [s, a, b]);
console.log("ok accessor");
