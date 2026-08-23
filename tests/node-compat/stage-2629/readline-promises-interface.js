const assert = require('assert');
const readline = require('node:readline/promises');

const writes = [];
const output = { write(value) { writes.push(value); } };
const input = { once(event, callback) { assert.strictEqual(event, 'line'); callback('answer'); } };
const interfaceObject = readline.createInterface({ input, output });
assert.strictEqual(typeof interfaceObject.question, 'function');
assert.strictEqual(typeof interfaceObject.close, 'function');
assert.strictEqual(typeof interfaceObject.pause, 'function');
assert.strictEqual(typeof interfaceObject.resume, 'function');
interfaceObject.question('prompt:').then((answer) => {
  assert.strictEqual(answer, 'answer');
  assert.deepStrictEqual(writes, ['prompt:']);
  interfaceObject.close();
  console.log('readline promises interface: ok');
});
