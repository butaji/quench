// Node compat: crypto.subtle AES-GCM encrypt/decrypt/exportKey (real backend via aes-gcm crate).
const c = require('node:crypto');
const s = c.subtle;
if (!s) throw new Error('no subtle');
if (typeof s.encrypt !== 'function') throw new Error('no encrypt');
if (typeof s.decrypt !== 'function') throw new Error('no decrypt');
if (typeof s.exportKey !== 'function') throw new Error('no exportKey');

(async () => {
  // 32-byte AES-256 key
  const keyBytes = new Uint8Array(32);
  for (let i = 0; i < 32; i++) keyBytes[i] = i;
  const key = await s.importKey('raw', keyBytes, { name: 'AES-GCM' }, true, ['encrypt', 'decrypt', 'exportKey']);
  if (key.type !== 'secret') throw new Error('key.type=' + key.type);
  if (!key.algorithm || key.algorithm.name !== 'AES-GCM') throw new Error('key.algorithm=' + JSON.stringify(key.algorithm));

  // Fixed IV for reproducibility
  const iv = new Uint8Array(12);
  for (let i = 0; i < 12; i++) iv[i] = i + 1;

  const plaintext = new TextEncoder().encode('quench AES-GCM round-trip');
  const ct = await s.encrypt({ name: 'AES-GCM', iv }, key, plaintext);
  if (!(ct instanceof Uint8Array)) throw new Error('encrypt not bytes');
  if (ct.byteLength !== plaintext.byteLength + 16) throw new Error('AES-GCM ct length=' + ct.byteLength);

  // Decrypt and verify the round-trip
  const pt = await s.decrypt({ name: 'AES-GCM', iv }, key, ct);
  const decoded = new TextDecoder().decode(pt);
  if (decoded !== 'quench AES-GCM round-trip') throw new Error('decrypt mismatch: ' + decoded);

  // Different IV gives different ciphertext
  const iv2 = new Uint8Array(12);
  for (let i = 0; i < 12; i++) iv2[i] = i + 2;
  const ct2 = await s.encrypt({ name: 'AES-GCM', iv: iv2 }, key, plaintext);
  let same = ct.length === ct2.length;
  for (let i = 0; i < ct.length && same; i++) if (ct[i] !== ct2[i]) same = false;
  if (same) throw new Error('different iv gave identical ct');

  // exportKey raw returns the key bytes
  const exported = await s.exportKey('raw', key);
  if (!(exported instanceof Uint8Array)) throw new Error('exportKey not bytes');
  if (exported.byteLength !== 32) throw new Error('exportKey length=' + exported.byteLength);
  for (let i = 0; i < 32; i++) if (exported[i] !== keyBytes[i]) throw new Error('exported key byte ' + i);

  // Wrong IV fails to decrypt (auth tag mismatch)
  let failed = false;
  try {
    await s.decrypt({ name: 'AES-GCM', iv: iv2 }, key, ct);
  } catch (e) {
    failed = true;
  }
  if (!failed) throw new Error('decrypt with wrong iv should have failed');

  console.log('crypto-subtle-aes-gcm: ok');
})().catch(e => { console.error('crypto-subtle-aes-gcm FAIL:', e && (e.message || e)); process.exitCode = 1; });