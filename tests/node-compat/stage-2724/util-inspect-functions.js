const assert = require('assert');
const util = require('util');

function abc() {}
assert.strictEqual(util.inspect(abc), '[Function: abc]');
assert.strictEqual(util.inspect(() => 1), '[Function (anonymous)]');
assert.strictEqual(util.inspect(function* () {}), '[GeneratorFunction (anonymous)]');
assert.strictEqual(util.inspect(async function* named() {}), '[AsyncGeneratorFunction: named]');
assert.strictEqual(util.inspect(/foo(bar\n)?/gi), '/foo(bar\\n)?/gi');
assert.strictEqual(util.inspect(new Date('2010-02-14T11:48:40.000Z')), '2010-02-14T11:48:40.000Z');
assert.strictEqual(util.inspect('\n\x01'), "'\\n\\x01'");
assert.strictEqual(util.inspect([1, 2, 3], true), '[ 1, 2, 3, [length]: 3 ]');
assert.ok(util.inspect(new Uint8Array(0), { showHidden: true }).includes('[buffer]'));
assert.strictEqual(
  util.inspect({ a: { b: { c: { d: 2 } } } }, false, null),
  '{\n  a: { b: { c: { d: 2 } } }\n}'
);
const buffer = new Uint8Array([1, 2, 3, 4]).buffer;
assert.strictEqual(
  util.inspect(buffer),
  'ArrayBuffer { [Uint8Contents]: <01 02 03 04>, [byteLength]: 4 }'
);
assert.strictEqual(
  util.inspect(new DataView(buffer, 1, 2)),
  'DataView {\n  [byteLength]: 2,\n  [byteOffset]: 1,\n  [buffer]: ArrayBuffer { [Uint8Contents]: <01 02 03 04>, [byteLength]: 4 }\n}'
);
assert.strictEqual(
  util.inspect(new ArrayBuffer(3), { showHidden: true, maxArrayLength: 2 }),
  'ArrayBuffer { [Uint8Contents]: <00 00 ... 1 more byte>, [byteLength]: 3 }'
);
const typed = new Float32Array(new ArrayBuffer(8));
typed[0] = 65;
typed[1] = 97;
assert.strictEqual(
  util.inspect(typed, { showHidden: true }),
  'Float32Array(2) [\n' +
    '  65,\n' +
    '  97,\n' +
    '  [BYTES_PER_ELEMENT]: 4,\n' +
    '  [length]: 2,\n' +
    '  [byteLength]: 8,\n' +
    '  [byteOffset]: 0,\n' +
    '  [buffer]: ArrayBuffer { [byteLength]: 8 }\n' +
    ']'
);
const accessors = {};
Object.defineProperty(accessors, 'readonly', { get() { return 1; } });
Object.defineProperty(accessors, 'writeonly', { set() {} });
assert.strictEqual(util.inspect(accessors), '{ readonly: [Getter], writeonly: [Setter] }');
const circular = {};
circular.a = circular;
assert.strictEqual(util.inspect(circular), '<ref *1> { a: [Circular *1] }');

console.log('util.inspect function values: ok');
