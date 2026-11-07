// new Date takes its prototype from NewTarget after converting its arguments: a Date made in
// another realm has that realm's Date.prototype, a subclass instance its subclass prototype,
// and a NewTarget whose prototype is not an object falls back to NewTarget's realm Date.prototype.
const assert = require("assert");
const vm = require("vm");

const context = vm.createContext({});
const made = vm.runInContext("new Date(0)", context);
assert.notStrictEqual(Object.getPrototypeOf(made), Date.prototype);
assert.strictEqual(made instanceof Date, false);
assert.strictEqual(vm.runInContext("Object.getPrototypeOf(new Date(1)) === Date.prototype", context), true);
assert.strictEqual(Date.prototype.getTime.call(made), 0);
assert.strictEqual(vm.runInContext("var d = new Date(2020, 1, 3); d.getMonth() + ':' + d.getDate()", context), "1:3");

const ForeignDate = vm.runInContext("Date", context);
assert.strictEqual(Object.getPrototypeOf(new ForeignDate(5)), ForeignDate.prototype);
assert.strictEqual(Object.getPrototypeOf(Reflect.construct(Date, [5], ForeignDate)), ForeignDate.prototype);
assert.strictEqual(Object.getPrototypeOf(Reflect.construct(ForeignDate, [5])), ForeignDate.prototype);

class Stamp extends Date {
  label() { return "t" + this.getTime(); }
}
const stamp = new Stamp(7);
assert.strictEqual(Object.getPrototypeOf(stamp), Stamp.prototype);
assert.strictEqual(stamp.label(), "t7");
assert.strictEqual(stamp instanceof Date, true);

// A NewTarget whose prototype is not an object: fall back to NewTarget's realm %Date.prototype%.
const bare = vm.runInContext("(function () {})", context);
bare.prototype = 1;
assert.strictEqual(Object.getPrototypeOf(Reflect.construct(Date, [0], bare)), ForeignDate.prototype);

// The prototype is read after the arguments are converted.
const order = [];
const target = function () {}.bind();
Object.defineProperty(target, "prototype", { get() { order.push("prototype"); return Date.prototype; } });
Reflect.construct(Date, [{ valueOf() { order.push("valueOf"); return 3; } }], target);
assert.deepStrictEqual(order, ["valueOf", "prototype"]);

// Many constructions in a foreign realm keep working after the realm's caches are warm.
const total = vm.runInContext("var s = 0; for (var i = 0; i < 300; i++) { var o = { k: i }; s += new Date(i).getTime() + o.k; } s", context);
assert.strictEqual(total, 2 * (299 * 300) / 2);
console.log("ok");
