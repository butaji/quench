// Node compat: crypto.subtle.digest accepts an Algorithm object.
const c = require('node:crypto');
const subtle = c.subtle;
if (!subtle) throw new Error('no subtle');

(async () => {
  const data = new TextEncoder().encode('abc');
  const digest = await subtle.digest({ name: 'SHA-256' }, data);
  if (!(digest instanceof Uint8Array)) throw new Error('digest not bytes');
  if (digest.byteLength !== 32) throw new Error('digest length=' + digest.byteLength);
  const expected = 'ba7816bf8f01cfea414140de5dae2223' +
    'b00361a396177a9cb410ff61f20015ad';
  const actual = Buffer.from(digest).toString('hex');
  if (actual !== expected) throw new Error('digest=' + actual);

  // WebCrypto algorithm names are ASCII case-insensitive.
  const mixed = await subtle.digest({ name: 'sHa-256' }, data);
  if (Buffer.from(mixed).toString('hex') !== expected)
    throw new Error('mixed-case digest mismatch');

  console.log('crypto-subtle-digest: ok');
})().catch(e => {
  console.error('crypto-subtle-digest FAIL:', e && (e.message || e));
  process.exitCode = 1;
});
