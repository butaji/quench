// Arrays made by literals, natives and callback methods take the Array.prototype of the realm
// that makes them, whichever realm is active; array callback methods pass their callback the
// element, the index and the array, which a sloppy callback's arguments and legacy reflection
// observe.
const assert = require("assert");
const vm = require("vm");

const context = vm.createContext({});
const made = vm.runInContext("var make = function (x) { return [x, x + 1]; }; make(1)", context);
assert.notStrictEqual(Object.getPrototypeOf(made), Array.prototype);
assert.strictEqual(made instanceof Array, false);
assert.strictEqual(Array.isArray(made), true);
assert.strictEqual(vm.runInContext("Object.getPrototypeOf(make(2)) === Array.prototype", context), true);
const make = vm.runInContext("make", context);
assert.notStrictEqual(Object.getPrototypeOf(make(3)), Array.prototype);
const foreignMap = vm.runInContext("Array.prototype.map", context);
const mapped = foreignMap.call([1, 2], (v) => v * 2);
assert.notStrictEqual(Object.getPrototypeOf(mapped), Array.prototype);
assert.strictEqual(mapped.join(), "2,4");
const nested = [1, 2, 3].map((v) => [v]);
assert.strictEqual(Object.getPrototypeOf(nested), Array.prototype);
assert.strictEqual(Object.getPrototypeOf(nested[0]), Array.prototype);
assert.strictEqual(
  vm.runInContext("[1, 2].map(function (v) { return [v]; }).every(function (a) { return Object.getPrototypeOf(a) === Array.prototype; })", context),
  true,
);
assert.strictEqual(Object.getPrototypeOf([..."ab"]), Array.prototype);
assert.strictEqual(Object.getPrototypeOf(JSON.parse("[1]")), Array.prototype);
assert.strictEqual(
  vm.runInContext("Object.getPrototypeOf(JSON.parse('[1]')) === Array.prototype && Object.getPrototypeOf('a,b'.split(',')) === Array.prototype", context),
  true,
);

const cells = [0];
function add(x) { cells[0] = (cells[0] + x) | 0; }
for (let i = 0; i < 100; i++) [i].forEach(add);
const sixtyFour = [];
for (let j = 0; j < 64; j++) sixtyFour.push(j);
sixtyFour.forEach(add);
const seen = [];
function sloppy(v, k, array) { seen.push(arguments.length + ":" + k + ":" + array.length + ":" + sloppy.arguments.length); }
[7, 8].forEach(sloppy);
function one(v) { seen.push(typeof one.arguments[2]); }
[7, 8].forEach(one);
function few() { seen.push(arguments[0] + "/" + arguments[1]); }
[5, 6].forEach(few);
assert.strictEqual(cells[0], 6966);
assert.deepStrictEqual(seen, ["3:0:2:3", "3:1:2:3", "object", "object", "5/0", "6/1"]);
