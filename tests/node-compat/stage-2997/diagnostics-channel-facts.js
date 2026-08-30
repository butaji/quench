'use strict';

const assert = require('assert');
const diagnostics = require('diagnostics_channel');

const channel = diagnostics.channel('stage-2997');
assert.ok(channel instanceof diagnostics.Channel);
let seen = 0;
const subscriber = (message) => {
  seen++;
  assert.deepStrictEqual(message, { ok: true });
};
channel.subscribe(subscriber);
assert.strictEqual(channel.hasSubscribers, true);
channel.publish({ ok: true });
assert.strictEqual(seen, 1);
assert.strictEqual(channel.unsubscribe(subscriber), true);
assert.strictEqual(channel.hasSubscribers, false);
console.log('ok');
