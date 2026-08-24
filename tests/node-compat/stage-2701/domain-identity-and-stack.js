const assert = require("assert");
const domain = require("domain");
const EventEmitter = require("events");

const d = domain.create();
const first = new EventEmitter();
const second = new EventEmitter();

d.add(first).add(second);
assert.strictEqual(first.domain, d);
assert.strictEqual(second.domain, d);
assert.strictEqual(d.members.length, 2);
d.remove(second);
assert.strictEqual(d.members.length, 1);
assert.strictEqual(second.domain, undefined);

const nested = domain.create();
d.enter();
nested.enter();
assert.deepStrictEqual(domain._stack, [d, nested]);
d.exit();
assert.deepStrictEqual(domain._stack, []);
