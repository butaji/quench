const assert = require("assert");
const domain = require("domain");
const EventEmitter = require("events");

const d = domain.create();
const emitter = new EventEmitter();
d.add(emitter);
assert.strictEqual(emitter.domain, d);
assert.strictEqual(
  Object.prototype.propertyIsEnumerable.call(emitter, "domain"),
  false,
);
d.remove(emitter);
assert.notStrictEqual(emitter.domain, d);

d.name = "d";
d.enter();
assert.deepStrictEqual(domain._stack, [d]);
d.exit();
assert.deepStrictEqual(domain._stack, []);

console.log("domain membership stack passed");
