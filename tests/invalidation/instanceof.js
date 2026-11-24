// instanceof folded under the constructor's prototype and @@hasInstance.
function C() {}
const c = new C();
function test(x) { return x instanceof C; }
let n = 0;
for (let k = 0; k < 20000; k++) if (test(c)) n++;
C.prototype = {};
const a = test(c);
Object.defineProperty(C, Symbol.hasInstance, { value: () => true });
const b = test({});
if (n !== 20000 || a !== false || b !== true) throw new Error("instanceof: " + [n, a, b]);
console.log("ok instanceof");
