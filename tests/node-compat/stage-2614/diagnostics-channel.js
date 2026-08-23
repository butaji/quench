const assert = require('assert');
const diagnostics = require('diagnostics_channel');

const channel = diagnostics.channel('compatibility-test');
let seen;
function subscriber(message, name) { seen = { message, name }; }
channel.subscribe(subscriber);
assert.strictEqual(channel.hasSubscribers, true);
channel.publish({ value: 42 });
assert.deepStrictEqual(seen, { message: { value: 42 }, name: 'compatibility-test' });
assert.strictEqual(channel.unsubscribe(subscriber), true);
assert.strictEqual(channel.hasSubscribers, false);
console.log('diagnostics channel: ok');
