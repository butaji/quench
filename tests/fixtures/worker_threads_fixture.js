const path = require('path');
const { Worker } = require('worker_threads');
const child = path.join(__dirname, 'worker_threads_child.js');
const worker = new Worker(child, { workerData: { answer: 42 } });
let received = false;
worker.on('message', (message) => {
  if (message.workerData.answer !== 42 || message.isMainThread !== false) process.exitCode = 1;
  received = true;
});
worker.on('exit', (code) => {
  if (code !== 0 || !received) process.exitCode = 1;
  console.log(JSON.stringify({ received, code }));
});
