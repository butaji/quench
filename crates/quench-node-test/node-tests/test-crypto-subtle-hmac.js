// Node compat: crypto.subtle HMAC sign/verify (real backend via pure-Rust HMAC-SHA256).
const c = require('node:crypto');
const s = c.subtle;
if (!s) throw new Error('no subtle');
if (typeof s.sign !== 'function') throw new Error('no sign');
if (typeof s.verify !== 'function') throw new Error('no verify');

(async () => {
  const keyBytes = new TextEncoder().encode('key');
  const k = await s.importKey('raw', keyBytes,
    { name: 'HMAC', hash: 'SHA-256' }, false, ['sign', 'verify']);
  if (k.extractable !== false) throw new Error('extractable=' + k.extractable);
  if (k.usages.length !== 2 || k.usages[0] !== 'sign' || k.usages[1] !== 'verify') {
    throw new Error('usages=' + k.usages);
  }
  if (k.algorithm.name !== 'HMAC') throw new Error('algo.name=' + k.algorithm.name);
  if (k.algorithm.hash !== 'SHA-256') throw new Error('algo.hash=' + k.algorithm.hash);

  const msg = new TextEncoder().encode('message');
  const sig = await s.sign('HMAC', k, msg);
  if (!(sig instanceof Uint8Array)) throw new Error('sign not bytes');
  if (sig.byteLength !== 32) throw new Error('sign length=' + sig.byteLength);

  // RFC 4231 Test Case 1: key = 0x0b * 20, data = "Hi There", SHA-256 HMAC
  //   = b0 34 4c 61 d8 db 38 53 5c a8 01 af 21 1f 6c 23
  //     3a 3f ab ad 99 21 9c 41 93 63 81 70 84 be a4 87
  const rfcKey = await s.importKey('raw', new Uint8Array(20).fill(0x0b),
    { name: 'HMAC', hash: 'SHA-256' }, false, ['sign', 'verify']);
  const rfcSig = await s.sign('HMAC', rfcKey, new TextEncoder().encode('Hi There'));
  const rfcExpected = [
    0xb0,0x34,0x4c,0x61,0xd8,0xdb,0x38,0x53, 0x5c,0xa8,0x01,0xaf,0x21,0x1f,0x6c,0x23,
    0x3a,0x3f,0xab,0xad,0x99,0x21,0x9c,0x41, 0x93,0x63,0x81,0x70,0x84,0xbe,0xa4,0x87,
  ];
  for (let i = 0; i < 32; i++) {
    if (rfcSig[i] !== rfcExpected[i]) throw new Error('RFC4231 byte ' + i + ': got ' + rfcSig[i].toString(16));
  }

  const ok = await s.verify('HMAC', k, sig, msg);
  if (ok !== true) throw new Error('verify ok=' + ok);
  const bad = await s.verify('HMAC', k, sig, new TextEncoder().encode('different'));
  if (bad !== false) throw new Error('verify different=' + bad);
  // Verify with tampered signature
  const tampered = new Uint8Array(sig); tampered[0] ^= 0xff;
  const reject = await s.verify('HMAC', k, tampered, msg);
  if (reject !== false) throw new Error('verify tampered=' + reject);

  console.log('crypto-subtle-hmac: ok');
})().catch(e => { console.error('crypto-subtle-hmac FAIL:', e && (e.message || e)); process.exitCode = 1; });