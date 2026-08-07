const { MessageChannel } = require("worker_threads");
const channel = new MessageChannel();

if (channel.port1.hasRef()) throw new Error("new port was refed");
if (channel.port1.ref() !== channel.port1 || !channel.port1.hasRef()) {
  throw new Error("ref() did not retain the port");
}
if (channel.port1.unref() !== channel.port1 || channel.port1.hasRef()) {
  throw new Error("unref() did not release the port");
}
