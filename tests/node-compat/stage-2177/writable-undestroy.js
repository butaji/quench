const { Writable } = require("stream");

let finals = 0;
const writable = new Writable({
  write: () => {},
  final(callback) {
    finals++;
    callback();
  },
  autoDestroy: true
});

writable.end();
writable.once("close", () => {
  writable._undestroy();
  writable.once("finish", () => {
    if (finals !== 2) throw new Error(`expected two finals, got ${finals}`);
  });
  writable.end();
});
