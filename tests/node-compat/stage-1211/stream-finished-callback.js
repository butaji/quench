const assert = require("assert");
const { finished } = require("stream");

const createEmitter = () => {
  const listeners = new Map();
  return {
    once(event, listener) {
      listeners.set(event, listener);
    },
    emit(event, value) {
      listeners.get(event)?.(value);
    },
  };
};

const readable = createEmitter();
let readableDone = false;
const cleanupReadable = finished(readable, () => (readableDone = true));
readable.emit("end");
cleanupReadable();
assert(readableDone);

const writable = createEmitter();
let writableDone = false;
finished(writable, () => (writableDone = true));
writable.emit("finish");
assert(writableDone);
