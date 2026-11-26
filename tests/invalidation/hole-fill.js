// Hole fills in place (a store into a hole of a dense array): a prototype index setter, a non-extensible
// array and a changed prototype each take the general [[Set]] once a row has filled holes in place.
function put(a, i, v) { a[i] = v; }
function fill(make) {
  let filled = 0;
  for (let r = 0; r < 300; r++) {
    const a = make();
    for (let i = 0; i < 8; i++) { try { put(a, i, { i }); } catch (e) { } }
    filled = 0; for (let i = 0; i < 8; i++) if (Object.prototype.hasOwnProperty.call(a, i)) filled++;
  }
  return filled;
}
const plain = fill(() => new Array(8));
const sealed = fill(() => Object.seal(new Array(8)));
const prevented = fill(() => Object.preventExtensions(new Array(8)));
let hits = 0;
Object.defineProperty(Array.prototype, 3, { set(v) { hits++; }, configurable: true });
const setter = fill(() => new Array(8));
delete Array.prototype[3];
const proto = Object.create(Array.prototype, { 5: { set(v) { hits += 1000; } } });
const custom = fill(() => { const a = new Array(8); Object.setPrototypeOf(a, proto); return a; });
const again = fill(() => new Array(8));
const result = [plain, sealed, prevented, setter, custom, again, hits].join();
if (result !== "8,0,0,7,7,8,300300") throw new Error("hole fill: " + result);
console.log("ok hole-fill");
