'use strict';

const fs = require('node:fs');
const promises = require('node:fs/promises');
const path = require('node:path');
const Buffer = require('node:buffer').Buffer;
if (!process.argv[2]) throw new Error('Pass a scratch directory as argv[2]');
const directory = path.join(process.argv[2], 'vfs-parity-fs');
const file = path.join(directory, 'file');

async function main() {
  fs.mkdirSync(directory, { recursive: true });
  let handle;
  try {
    handle = await promises.open(file, 'w+');
    let closeEvents = 0;
    handle.on('close', () => { closeEvents++; });
    await handle.writeFile(Buffer.from('hello'));
    await handle.sync();
    const written = await handle.write(Buffer.from('!'), 0, 1, 5);
    await handle.datasync();
    const buffer = Buffer.alloc(6);
    const read = await handle.read(buffer, 0, buffer.length, 0);
    const stat = await handle.stat();
    await handle.close();
    const closedFd = handle.fd;
    let closedRead;
    try {
      await handle.read(Buffer.alloc(1), 0, 1, 0);
      closedRead = { succeeded: true };
    } catch (error) {
      closedRead = { name: error.name, code: error.code };
    }
    handle = undefined;
    await promises.utimes(file, 946684800, 946684802);
    const metadata = fs.statSync(file);
    console.log(JSON.stringify({
      promisesIdentity: promises === fs.promises,
      builtinIdentity: process.getBuiltinModule('fs') === fs,
      written: written.bytesWritten,
      writeBufferIdentity: Buffer.isBuffer(written.buffer),
      bytesRead: read.bytesRead,
      readBufferIdentity: read.buffer === buffer,
      text: buffer.toString(),
      isFile: stat.isFile(),
      size: stat.size,
      closedFd,
      closeEvents,
      closedRead,
      diskText: fs.readFileSync(file, 'utf8'),
      atimeMs: Math.trunc(metadata.atimeMs),
      mtimeMs: Math.trunc(metadata.mtimeMs),
    }));
  } finally {
    if (handle) await handle.close();
    fs.rmSync(directory, { recursive: true, force: true });
  }
}

main().catch((error) => {
  console.error(JSON.stringify({ name: error.name, code: error.code, message: error.message }));
  process.exitCode = 1;
});
