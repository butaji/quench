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
let md = crypto.createHash('md5'); md = md.update('abc');
assert.strictEqual(md.digest('hex'), '900150983cd24fb0d6963f7d28e17f72');
let s4 = crypto.createHash('sha512'); s4 = s4.update('abc');
assert.strictEqual(
  s4.digest('hex'),
  'ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f'
);
assert.strictEqual(crypto.randomUUID().length, 36);
const randomValue = crypto.randomInt(2, 5);
assert.ok(randomValue >= 2 && randomValue < 5);