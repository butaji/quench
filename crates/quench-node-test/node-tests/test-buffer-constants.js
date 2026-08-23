// Node compat: Buffer.constants and kStringMaxLength (Bun-green).
const { Buffer } = require('node:buffer');
if (Buffer.kStringMaxLength !== 536870888) throw new Error('kStringMaxLength: ' + Buffer.kStringMaxLength);
if (Buffer.kMaxLength !== 9007199254740991) throw new Error('kMaxLength: ' + Buffer.kMaxLength);
if (typeof Buffer.constants !== 'object' || Buffer.constants === null) {
  throw new Error('Buffer.constants missing');
}
if (Buffer.constants.MAX_LENGTH !== 9007199254740991) {
  throw new Error('MAX_LENGTH: ' + Buffer.constants.MAX_LENGTH);
}
if (Buffer.constants.MAX_STRING_LENGTH !== 536870888) {
  throw new Error('MAX_STRING_LENGTH: ' + Buffer.constants.MAX_STRING_LENGTH);
}
console.log('buffer-constants: ok');