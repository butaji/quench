const { EventEmitter } = require("events");

const previous = EventEmitter.captureRejections;
EventEmitter.captureRejections = true;
const emitter = new EventEmitter({ captureRejections: false });
EventEmitter.captureRejections = previous;

if (emitter.captureRejections !== false) {
  throw new Error(
    "explicit instance option should override the static default",
  );
}
