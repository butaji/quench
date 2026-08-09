const assert = require("assert");
const { Readable, Writable, finished } = require("stream");

const readable = new Readable({ read() {} });
finished(readable, (error) => assert.ifError(error));
readable.push(null);
readable.resume();

const writable = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  }
});
finished(writable, (error) => assert.ifError(error));
writable.end();
