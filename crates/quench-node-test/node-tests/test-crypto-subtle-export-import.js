// Regression: node:crypto exposes callable WebCrypto exportKey/importKey.
const crypto = require('node:crypto');
const subtle = crypto.subtle;
if (!subtle || typeof subtle.generateKey !== 'function' || typeof subtle.importKey !== 'function' || typeof subtle.exportKey !== 'function') {
  throw new Error('crypto.subtle export/import surface is not callable');
}
if (!crypto.webcrypto || crypto.webcrypto.subtle !== subtle || typeof crypto.webcrypto.subtle.exportKey !== 'function') {
  throw new Error('crypto.webcrypto.subtle exportKey surface is not callable');
}
(async () => {
  const key = await subtle.generateKey({ name: 'AES-GCM', length: 128 }, true, ['encrypt', 'decrypt']);
  const raw = await subtle.exportKey('raw', key);
  if (raw.byteLength !== 16) throw new Error('unexpected raw key length: ' + raw.byteLength);
  const imported = await subtle.importKey('raw', raw, { name: 'AES-GCM' }, true, ['encrypt', 'decrypt']);
  if (imported.type !== 'secret' || imported.algorithm.name !== 'AES-GCM') throw new Error('imported key metadata mismatch');
  console.log('crypto-subtle-export-import: ok');
})().catch((error) => { console.error('crypto-subtle-export-import FAIL:', error && (error.message || error)); process.exitCode = 1; });
