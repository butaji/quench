'use strict';

const childProcess = require('node:child_process');
const Buffer = require('node:buffer').Buffer;
const childArguments = ['-e', 'process.stdout.write("child output");'];
const spawned = childProcess.spawnSync(process.execPath, childArguments);
const executed = childProcess.execFileSync(process.execPath, childArguments);
console.log(JSON.stringify({
  status: spawned.status,
  signal: spawned.signal,
  errorCode: spawned.error ? spawned.error.code : null,
  stdoutBuffer: Buffer.isBuffer(spawned.stdout),
  stdoutText: spawned.stdout.toString(),
  stderrBuffer: Buffer.isBuffer(spawned.stderr),
  stderrBytes: spawned.stderr.length,
  execBuffer: Buffer.isBuffer(executed),
  execText: executed.toString(),
}));
