const { parentPort, workerData, isMainThread } = require('worker_threads');
if (isMainThread) throw new Error('expected isolated worker');
parentPort.postMessage({ workerData, isMainThread });
