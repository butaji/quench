const assert = require("assert");
const { PassThrough } = require("stream");
const { blob, text } = require("stream/consumers");

const binary = new PassThrough({ objectMode: true });
let blobSize;
blob(binary).then((value) => {
  blobSize = value.size;
});
binary.write({});
binary.end({});

const textual = new PassThrough({ objectMode: true });
let textError;
text(textual).catch((error) => {
  textError = error;
});
textual.write({});
textual.end({});

process.on("beforeExit", () => {
  assert.strictEqual(blobSize, 30);
  assert.strictEqual(textError?.code, "ERR_INVALID_ARG_TYPE");
});
