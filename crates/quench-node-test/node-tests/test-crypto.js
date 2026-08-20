import assert from 'node:assert';
import crypto from 'node:crypto';

const bytes = crypto.randomBytes(32);
assert.strictEqual(bytes.length, 32);
assert.ok(bytes.some((value) => value !== 0));
const target = Buffer.alloc(24);
const result = crypto.randomFillSync(target, 4, 12);
assert.strictEqual(result, target);
assert.ok(target.subarray(4, 16).some((value) => value !== 0));
assert.deepStrictEqual([...target.subarray(0, 4)], [0, 0, 0, 0]);
assert.throws(() => crypto.randomBytes(-1), /size/);
for (const name of ['createHash', 'createCipheriv', 'createDecipheriv', 'generateKeyPairSync']) {
  assert.throws(() => crypto[name](), /not supported/);
}
console.log('crypto fixtures ok');
