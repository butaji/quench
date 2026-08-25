"use strict";

const assert = require("assert");
const { Worker, MessageChannel } = require("worker_threads");
console.log(typeof DOMException);

const channel = new MessageChannel();
assert.throws(() => new Worker("", {
  eval: true,
  workerData: { port: channel.port1 },
  transferList: [],
}), {
  constructor: DOMException,
  name: "DataCloneError",
  code: 25,
  message: "Object that needs transfer was found in message but not listed in transferList",
});
