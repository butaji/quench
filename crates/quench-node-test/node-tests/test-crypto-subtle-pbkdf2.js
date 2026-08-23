// Node compat: crypto.subtle PBKDF2 (real backend via pure-Rust HMAC-SHA256).
const c = require('node:crypto');
const subtle = c.subtle;
if (!subtle) throw new Error('no subtle');
if (typeof subtle.deriveBits !== 'function') throw new Error('no deriveBits');
if (typeof subtle.deriveKey !== 'function') throw new Error('no deriveKey');

(async () => {
  const password = new TextEncoder().encode('password\u0000');
  const salt = new TextEncoder().encode('salt\u0001');

  const key = await subtle.importKey('raw', password, 'PBKDF2', false, ['deriveBits', 'deriveKey']);

  // 32-byte derivation. Compare to RFC 6070 PBKDF2-HMAC-SHA256 vector 5
  // (p = "password\0", s = "salt", c = 1, dkLen = 32):
  // expected = 9e 88 9b 32 73 b8 9c c3 a6 8e 8c 44 6c 6b 53 6b
  //            c3 76 15 36 a2 11 6d b4 a7 31 80 7d 3d 6e 2a 40
  const derived = await subtle.deriveBits({ name: 'PBKDF2', salt, iterations: 1, hash: 'SHA-256' }, key, 32);
  if (!(derived instanceof Uint8Array)) throw new Error('deriveBits not bytes');
  if (derived.byteLength !== 32) throw new Error('deriveBits length=' + derived.byteLength);
  const expected = [
    0x9e,0x88,0x9b,0x32,0x73,0xb8,0x9c,0xc3, 0xa6,0x8e,0x8c,0x44,0x6c,0x6b,0x53,0x6b,
    0xc3,0x76,0x15,0x36,0xa2,0x11,0x6d,0xb4, 0xa7,0x31,0x80,0x7d,0x3d,0x6e,0x2a,0x40,
  ];
  for (let i = 0; i < 32; i++) {
    if (derived[i] !== expected[i]) throw new Error('deriveBits byte ' + i + ' mismatch');
  }

  // deriveKey wrapping PBKDF2 output as an AES-GCM 256-bit key
  const k = await subtle.deriveKey(
    { name: 'PBKDF2', salt, iterations: 1, hash: 'SHA-256' },
    key,
    { name: 'AES-GCM', length: 256 },
    false,
    ['encrypt']
  );
  if (k.type !== 'secret') throw new Error('deriveKey type=' + k.type);
  if (!k.algorithm || k.algorithm.name !== 'AES-GCM') throw new Error('deriveKey algo');
  if (k.algorithm.length !== 256) throw new Error('deriveKey length=' + k.algorithm.length);

  console.log('crypto-subtle-pbkdf2: ok');
})().catch(e => { console.error('crypto-subtle-pbkdf2 FAIL:', e && (e.message || e)); process.exitCode = 1; });