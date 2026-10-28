'use strict';

const bufferModule = require('node:buffer');
const Buffer = bufferModule.Buffer;
const payload = Buffer.from('Buffer and web compression preserve bytes: \u0000\u03bb');

async function transform(Constructor, input) {
  const stream = new Constructor('gzip');
  const output = (async () => {
    const chunks = [];
    const reader = stream.readable.getReader();
    for (;;) {
      const item = await reader.read();
      if (item.done) break;
      chunks.push(Buffer.from(item.value));
    }
    reader.releaseLock();
    return Buffer.concat(chunks);
  })();
  const writer = stream.writable.getWriter();
  await writer.write(new Uint8Array(input));
  await writer.close();
  writer.releaseLock();
  return output;
}

async function main() {
  const compressed = await transform(CompressionStream, payload);
  const restored = await transform(DecompressionStream, compressed);
  console.log(JSON.stringify({
    globalBufferIdentity: globalThis.Buffer === Buffer,
    builtinBufferIdentity: process.getBuiltinModule('buffer').Buffer === Buffer,
    gzipHeader: [...compressed.subarray(0, 3)],
    restoredBuffer: Buffer.isBuffer(restored),
    restoredHex: restored.toString('hex'),
    restoredText: restored.toString(),
    payloadHex: payload.toString('hex'),
  }));
}

main().catch((error) => {
  console.error(JSON.stringify({ name: error.name, code: error.code, message: error.message }));
  process.exitCode = 1;
});
