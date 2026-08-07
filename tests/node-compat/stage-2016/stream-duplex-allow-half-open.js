const assert = require("assert");
const { Duplex } = require("stream");

const make = (allowHalfOpen, writableEnded = false) => {
  const stream = new Duplex({ read() {}, allowHalfOpen });
  stream._writableState.ended = writableEnded;
  return stream;
};

const open = make(true);
let openFinished = false;
open.once("finish", () => {
  openFinished = true;
});
open.resume();
open.push(null);

const closed = make(false);
let closedFinished = false;
closed.once("finish", () => {
  closedFinished = true;
});
closed.resume();
closed.push(null);

const alreadyEnded = make(false, true);
let alreadyFinished = false;
alreadyEnded.once("finish", () => {
  alreadyFinished = true;
});
alreadyEnded.resume();
alreadyEnded.push(null);

setImmediate(() => {
  assert.strictEqual(openFinished, false);
  assert.strictEqual(closedFinished, true);
  assert.strictEqual(alreadyFinished, false);
  console.log("Duplex allowHalfOpen behavior passed");
});
