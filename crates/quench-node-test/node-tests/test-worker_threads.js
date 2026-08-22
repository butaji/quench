// Node compat: worker lifecycle and message-port queue behavior.
const wt = require('node:worker_threads');
if (typeof wt.Worker !== 'function') throw new Error('Worker: ' + typeof wt.Worker);
if (typeof wt.MessageChannel !== 'function') throw new Error('MessageChannel: ' + typeof wt.MessageChannel);
const channel = new wt.MessageChannel();
channel.port1.postMessage({ value: 7 });
channel.port1.close();
let received;
channel.port2.on('message', value => { received = value.value; });
if (received !== 7) throw new Error('queued message delivery');
let closed = false;
channel.port1.on('close', () => { closed = true; });
if (!closed) throw new Error('queued close delivery');
console.log('wt: ok');
