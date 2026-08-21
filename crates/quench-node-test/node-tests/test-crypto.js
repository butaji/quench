const assert = require('assert');
const crypto = require('node:crypto');

const bytes = crypto.randomBytes(32);
assert.strictEqual(bytes.length, 32);
let nonzero = false;
for (let i = 0; i < bytes.length; i++) if (bytes[i] !== 0) nonzero = true;
assert.ok(nonzero);
const target = Buffer.alloc(24);
const result = crypto.randomFillSync(target, 4, 12);
assert.strictEqual(result, target);
let filled = false;
for (let i = 4; i < 16; i++) if (target[i] !== 0) filled = true;
assert.ok(filled);
for (const name of ['createHash', 'createCipheriv', 'createDecipheriv', 'generateKeyPairSync']) {
  assert.strictEqual(typeof crypto[name], 'function');
}
console.log('crypto fixtures ok');
let hash = crypto.createHash('sha256');
hash = hash.update('abc');
assert.strictEqual(
  hash.digest('hex'),
  'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad',
);
let mac = crypto.createHmac('sha256', 'key');
mac = mac.update('The quick brown fox jumps over the lazy dog');
assert.strictEqual(mac.digest('hex'), 'f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8');
assert.strictEqual(crypto.randomUUID().length, 36);
const randomValue = crypto.randomInt(2, 5);
assert.ok(randomValue >= 2 && randomValue < 5);