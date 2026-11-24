// The intact fuses: each intrinsic replaced after hot code relied on it.
const results = [];
function spread(a) { let s = 0; for (const x of a) s += x; return s; }
for (let k = 0; k < 3000; k++) spread([1, 2, 3]);
const values = Array.prototype[Symbol.iterator];
Array.prototype[Symbol.iterator] = function* () { yield 100; };
results.push(spread([1, 2, 3]) === 100);
Array.prototype[Symbol.iterator] = values;
const it = Object.getPrototypeOf([][Symbol.iterator]());
const next = it.next;
for (let k = 0; k < 3000; k++) spread([1, 2]);
it.next = function () { return { done: true }; };
results.push(spread([1, 2]) === 0);
it.next = next;
function setSum(s) { let t = 0; for (const x of s) t += x; return t; }
function mapSum(m) { let t = 0; for (const [, v] of m) t += v; return t; }
for (let k = 0; k < 3000; k++) { setSum(new Set([1, 2])); mapSum(new Map([[1, 2]])); }
Set.prototype[Symbol.iterator] = function* () { yield 9; };
Map.prototype[Symbol.iterator] = function* () { yield [0, 9]; };
results.push(setSum(new Set([1, 2])) === 9, mapSum(new Map([[1, 2]])) === 9);
function chars(s) { let t = ""; for (const c of s) t += c; return t; }
function code(s) { return s.charCodeAt(0); }
function has(s) { return s.includes("b"); }
for (let k = 0; k < 3000; k++) { chars("ab"); code("a"); has("abc"); }
String.prototype[Symbol.iterator] = function* () { yield "q"; };
String.prototype.charCodeAt = () => -5;
String.prototype.includes = () => "no";
results.push(chars("ab") === "q", code("a") === -5, has("abc") === "no");
function rt(r, s) { return r.test(s); }
function rx(r, s) { return r.exec(s); }
for (let k = 0; k < 3000; k++) { rt(/a/, "a"); rx(/a/, "a"); }
const [test, exec] = [RegExp.prototype.test, RegExp.prototype.exec];
RegExp.prototype.test = () => "t";
RegExp.prototype.exec = () => null;
results.push(rt(/a/, "a") === "t", rx(/a/, "a") === null);
RegExp.prototype.test = test;
RegExp.prototype.exec = exec;
function mapped(a) { return a.map((x) => x).constructor === Array; }
for (let k = 0; k < 3000; k++) mapped([1]);
const arrayConstructor = Array.prototype.constructor;
class Other extends Array {}
const viaConstructor = [1];
viaConstructor.constructor = Other;
results.push(mapped(viaConstructor) === false && mapped([1]) === true);
Array.prototype.constructor = arrayConstructor;
function split(s) { return s.split(/,/).length; }
for (let k = 0; k < 3000; k++) split("a,b");
class R extends RegExp { constructor(...a) { super(...a); R.made = true; } }
Object.defineProperty(RegExp, Symbol.species, { get() { return R; } });
results.push(split("a,b") === 2 && R.made === true);
class Sub extends Array {}
Object.defineProperty(Array, Symbol.species, { get() { return Sub; } });
results.push(mapped([1]) === false);
const bad = results.findIndex((r) => !r);
if (bad !== -1) throw new Error("fuse " + bad + " " + results);
console.log("ok fuses");
