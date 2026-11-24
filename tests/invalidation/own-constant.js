// One object's own field read as a constant (an enum, a config): rewriting it retires the code.
const Config = { scale: 3, bias: 1 };
function f(x) { return x * Config.scale + Config.bias; }
let s = 0;
for (let k = 0; k < 20000; k++) s += f(1);
Config.scale = 10;
const a = f(1);
Object.defineProperty(Config, "bias", { get() { return 100; } });
const b = f(1);
if (s !== 80000 || a !== 11 || b !== 110) throw new Error("own constant: " + [s, a, b]);
console.log("ok own-constant");
