// Node compat: crypto.subtle generateKey for AES-GCM and HMAC (real CSPRNG via rand).
const c = require('node:crypto');
const s = c.subtle;
if (!s) throw new Error('no subtle');
if (typeof s.generateKey !== 'function') throw new Error('no generateKey');

(async () => {
  // AES-GCM 256
  const k1 = await s.generateKey({ name: 'AES-GCM', length: 256 }, true, ['encrypt', 'decrypt']);
  if (k1.type !== 'secret') throw new Error('AES-GCM key.type=' + k1.type);
  if (k1.algorithm.name !== 'AES-GCM') throw new Error('AES-GCM algo');
  if (k1.algorithm.length !== 256) throw new Error('AES-GCM length=' + k1.algorithm.length);
  const exp1 = await s.exportKey('raw', k1);
  if (exp1.byteLength !== 32) throw new Error('AES-GCM export len=' + exp1.byteLength);

  // AES-GCM 128
  const k128 = await s.generateKey({ name: 'AES-GCM', length: 128 }, true, ['encrypt']);
  const exp128 = await s.exportKey('raw', k128);
  if (exp128.byteLength !== 16) throw new Error('AES-GCM 128 len=' + exp128.byteLength);

  // HMAC SHA-256
  const k2 = await s.generateKey({ name: 'HMAC', hash: 'SHA-256', length: 256 }, true, ['sign', 'verify']);
  if (k2.type !== 'secret') throw new Error('HMAC type=' + k2.type);
  if (k2.algorithm.name !== 'HMAC') throw new Error('HMAC algo');
  if (k2.algorithm.hash !== 'SHA-256') throw new Error('HMAC hash');

  // Round-trip: generateKey -> encrypt -> decrypt -> original message
  const iv = new Uint8Array(12);
  for (let i = 0; i < 12; i++) iv[i] = i + 1;
  const msg = new TextEncoder().encode('generated key');
  const ct = await s.encrypt({ name: 'AES-GCM', iv }, k1, msg);
  const pt = await s.decrypt({ name: 'AES-GCM', iv }, k1, ct);
  const decoded = new TextDecoder().decode(pt);
  if (decoded !== 'generated key') throw new Error('gen-key round-trip mismatch: ' + decoded);

  // Sign/verify round-trip using generated HMAC key
  const sig = await s.sign('HMAC', k2, msg);
  const ok = await s.verify('HMAC', k2, sig, msg);
  if (ok !== true) throw new Error('HMAC gen-key verify=' + ok);

  // Two calls produce different keys (randomness sanity check)
  const ka = await s.generateKey({ name: 'AES-GCM', length: 256 }, true, ['encrypt']);
  const kb = await s.generateKey({ name: 'AES-GCM', length: 256 }, true, ['encrypt']);
  const ea = await s.exportKey('raw', ka);
  const eb = await s.exportKey('raw', kb);
  let same = ea.length === eb.length;
  for (let i = 0; i < ea.length && same; i++) if (ea[i] !== eb[i]) same = false;
  if (same) throw new Error('generateKey not random');

  console.log('crypto-subtle-generate-key: ok');
})().catch(e => { console.error('crypto-subtle-generate-key FAIL:', e && (e.message || e)); process.exitCode = 1; });