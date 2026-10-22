'use strict';

const fs = require('node:fs');
const path = require('node:path');
if (!process.argv[2]) throw new Error('Pass a scratch directory as argv[2]');
const directory = path.join(process.argv[2], 'vfs-parity-metadata');
const file = path.join(directory, 'file');
const link = path.join(directory, 'link');

fs.mkdirSync(directory, { recursive: true });
try {
  fs.writeFileSync(file, 'data');
  fs.symlinkSync('file', link);
  const methods = {};
  for (const name of ['utimesSync', 'lutimesSync', 'futimesSync', 'utimes', 'lutimes', 'futimes', 'rm', 'write']) {
    const descriptor = Object.getOwnPropertyDescriptor(fs, name);
    methods[name] = descriptor ? {
      type: typeof descriptor.value,
      writable: descriptor.writable,
      enumerable: descriptor.enumerable,
      configurable: descriptor.configurable,
    } : null;
  }
  fs.utimesSync(file, 946684800, 946684802);
  let linkTimes;
  try {
    fs.lutimesSync(link, 946684804, 946684806);
    linkTimes = { mtimeMs: Math.trunc(fs.lstatSync(link).mtimeMs) };
  } catch (error) {
    linkTimes = { name: error.name, code: error.code };
  }
  console.log(JSON.stringify({
    methods,
    targetMtimeMs: Math.trunc(fs.statSync(file).mtimeMs),
    linkTimes,
  }));
} finally {
  fs.rmSync(directory, { recursive: true, force: true });
}
