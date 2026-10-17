'use strict';
// Advanced child-process IPC: values that only the V8 codec carries (Maps, Sets, typed
// arrays, Buffers, BigInts, Dates, cycles, errors) round-trip through a forked child that
// runs this engine and through one that runs `node`, so both sides of the wire must agree.
const { fork } = require('child_process');
const util = require('util');

const show = (value) => util.inspect(value, { depth: 6, breakLength: Infinity });

if (process.argv[2] === 'child') {
  process.on('message', (message) => {
    if (message === 'done') return process.disconnect();
    process.send({ echo: message, seen: show(message) });
  });
  process.send({ ready: 'NODE_CHANNEL_SERIALIZATION_MODE' in process.env });
  return;
}

const cyclic = { name: 'cycle' };
cyclic.self = cyclic;
const offset = new Uint16Array(new ArrayBuffer(16), 2, 3).fill(7);
const messages = [
  'text', 42, -0, true, null, [1, , 3],
  { map: new Map([[1, 'one'], ['k', { v: true }]]), set: new Set(['a', 1n]) },
  { buffer: Buffer.from('buffer'), bytes: new Uint8Array([1, 2, 3]), offset, view: new DataView(new ArrayBuffer(4)).byteLength },
  { floats: new Float64Array([0.5, -1]), bigints: new BigInt64Array([1n, -1n]), big: 2n ** 70n },
  { date: new Date(1700000000000), regexp: /a+b/gi, error: Object.assign(new RangeError('bad'), { stack: 'stack' }) },
  cyclic,
  { cmd: 'user', payload: 'x'.repeat(70000) },
];

function run(label, execPath) {
  return new Promise((resolve) => {
    const child = fork(__filename, ['child'], { serialization: 'advanced', execPath });
    let index = 0;
    child.on('message', (message) => {
      if ('ready' in message) {
        console.log(label, 'child sees bootstrap variable', message.ready);
      } else {
        const echo = message.echo;
        console.log(label, 'child saw', message.seen.length > 200 ? `${message.seen.slice(0, 80)}...` : message.seen);
        console.log(label, 'parent got', show(echo).slice(0, 200),
          Buffer.isBuffer(echo?.buffer), echo?.self === echo, echo?.error instanceof RangeError);
      }
      if (index < messages.length) child.send(messages[index++]);
      else child.send('done');
    });
    child.on('exit', (code, signal) => {
      console.log(label, 'exit', code, signal);
      resolve();
    });
  });
}

(async () => {
  const errors = [];
  const child = fork(__filename, ['child'], { serialization: 'advanced' });
  for (const value of [() => 1, Symbol('s'), new WeakMap()]) {
    try { child.send(value); } catch (error) { errors.push(`${error.name}: ${error.message}`); }
  }
  child.send('done');
  console.log('unserializable', errors);
  await new Promise((resolve) => child.on('exit', resolve));
  await run('same-engine', process.execPath);
  await run('node', 'node');
})();
