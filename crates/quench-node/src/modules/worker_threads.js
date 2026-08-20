// Worker construction requires a second VM/context, which this embedding does not expose.
module.exports = {
  isMainThread: true, threadId: 0, workerData: null, parentPort: null, MessageChannel: class MessageChannel {},
  MessagePort: class MessagePort {},
  Worker: class Worker { constructor() { throw new Error('Worker is unavailable in the embedded runtime: no child VM context'); } },
  receiveMessageOnPort() { return undefined; },
  markAsUncloneable() {}, markAsUntransferable() {},
  setEnvironmentData() {}, getEnvironmentData() { return undefined; }
};
