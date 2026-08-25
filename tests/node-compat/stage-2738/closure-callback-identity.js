const assert = require('assert');
const seen = [];
function schedule(callback) {
  process.nextTick(() => callback());
}
schedule(() => seen.push('a'));
schedule(() => seen.push('b'));
schedule(() => seen.push('c'));
setImmediate(() => assert.deepStrictEqual(seen, ['a', 'b', 'c']));
