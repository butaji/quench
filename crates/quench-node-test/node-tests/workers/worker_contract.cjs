'use strict';
// Worker threads contract: the main thread prints one line per observation, in an order
// fixed by awaiting each scenario; workers only communicate through messages and stdio.
const { Worker, MessageChannel, MessagePort, receiveMessageOnPort, isMainThread, parentPort, workerData, threadId } = require('worker_threads');

if (!isMainThread) {
  const { mode } = workerData;
  if (mode === 'echo') {
    parentPort.on('message', (value) => {
      if (value === 'done') { parentPort.close(); return; }
      parentPort.postMessage({ echoed: value, threadId });
    });
  } else if (mode === 'port') {
    workerData.port.postMessage({ via: 'transferred port', argv: process.argv.slice(2), env: process.env.CONTRACT_VALUE });
    workerData.port.close();
  } else if (mode === 'atomics') {
    const view = new Int32Array(workerData.shared);
    parentPort.on('message', ({ id }) => {
      workerData.reply.postMessage({ id, square: id * id });
      Atomics.add(view, 0, 1);
      Atomics.notify(view, 0);
    });
  } else if (mode === 'throw') {
    throw new RangeError('worker failure', { cause: { nested: [1, 2] } });
  } else if (mode === 'exit') {
    process.on('exit', (code) => parentPort.postMessage(`exiting ${code}`));
    process.exit(3);
  } else if (mode === 'loop') {
    setInterval(() => {}, 1000);
    parentPort.postMessage('looping');
  } else if (mode === 'stdio') {
    process.stdout.write('to stdout\n');
    process.stderr.write('to stderr\n');
  }
  return;
}

const show = (...values) => console.log(...values.map((value) => typeof value === 'string' ? value : JSON.stringify(value)));
const exited = (worker) => new Promise((resolve) => worker.once('exit', resolve));
const start = (mode, options = {}) => new Worker(__filename, { ...options, workerData: { mode, ...options.workerData } });

(async () => {
  // Messages round-trip through a worker in order, with structured values.
  const echo = start('echo');
  const replies = [];
  echo.on('message', (value) => replies.push(value));
  const online = new Promise((resolve) => echo.once('online', resolve));
  for (const value of [1, 'two', { three: [3n.toString()] }, new Map([[1, { deep: true }]])]) echo.postMessage(value);
  await online;
  echo.postMessage('done');
  show('echo exit', await exited(echo), replies.map((reply) => [reply.threadId, reply.echoed instanceof Map ? [...reply.echoed] : reply.echoed]));

  // A transferred MessagePort works from the worker; argv and env reach it.
  const { port1, port2 } = new MessageChannel();
  const viaPort = new Promise((resolve) => port1.once('message', resolve));
  const portWorker = start('port', { workerData: { port: port2 }, transferList: [port2], argv: ['a', 2], env: { CONTRACT_VALUE: 'from env' } });
  show('port message', await viaPort, 'exit', await exited(portWorker));
  port1.close();

  // The main thread blocks in Atomics.wait while the worker answers on a port it reads synchronously.
  const shared = new SharedArrayBuffer(4);
  const view = new Int32Array(shared);
  const reply = new MessageChannel();
  const atomics = start('atomics', { workerData: { shared, reply: reply.port2 }, transferList: [reply.port2] });
  const answers = [];
  for (let id = 1; id <= 3; id++) {
    const before = Atomics.load(view, 0);
    atomics.postMessage({ id });
    const outcome = Atomics.wait(view, 0, before, 10000);
    answers.push([outcome === 'ok' || outcome === 'not-equal', receiveMessageOnPort(reply.port1)]);
  }
  show('atomics', answers, Atomics.load(view, 0), receiveMessageOnPort(reply.port1));
  atomics.terminate();
  show('atomics exit', await exited(atomics));

  // A worker's uncaught exception reaches the parent as a cloned error, then exit 1.
  const thrower = start('throw');
  const failure = await new Promise((resolve) => thrower.once('error', resolve));
  show('error', failure.constructor.name, failure.message, failure.cause, failure instanceof RangeError, 'exit', await exited(thrower));

  // process.exit runs the worker's exit listeners and ends only the worker.
  const exiting = start('exit');
  const exitMessage = new Promise((resolve) => exiting.once('message', resolve));
  show('exit', await exitMessage, await exited(exiting));

  // terminate() ends a worker that would otherwise stay alive.
  const looping = start('loop');
  await new Promise((resolve) => looping.once('message', resolve));
  show('terminate', await looping.terminate());

  // Worker stdio arrives on worker.stdout/stderr (not piped when requested).
  const stdio = start('stdio', { stdout: true, stderr: true });
  let out = '';
  let err = '';
  stdio.stdout.on('data', (chunk) => { out += chunk; });
  stdio.stderr.on('data', (chunk) => { err += chunk; });
  show('stdio exit', await exited(stdio), out, err);

  // eval workers and API shapes.
  const evaluated = new Worker('require("worker_threads").parentPort.postMessage([typeof require, process.argv[1]])', { eval: true });
  show('eval', await new Promise((resolve) => evaluated.once('message', resolve)), await exited(evaluated));
  show('shapes', typeof MessagePort, threadId, isMainThread, parentPort, workerData, evaluated.threadId > 0);
})();
