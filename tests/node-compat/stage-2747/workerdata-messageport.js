"use strict";

const assert = require("assert");
const { Worker, MessageChannel } = require("worker_threads");

const channel = new MessageChannel();
const worker = new Worker(
  "require('worker_threads').workerData.port.postMessage('ok');",
  {
    eval: true,
    workerData: { port: channel.port2 },
    transferList: [channel.port2],
  },
);
channel.port1.once("message", (value) => assert.strictEqual(value, "ok"));
