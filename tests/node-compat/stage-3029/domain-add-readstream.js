"use strict";

const assert = require("assert");
const fs = require("fs");
const domain = require("domain").create();

const stream = fs.createReadStream("stream for nonexistent file");
domain.add(stream);
assert.strictEqual(stream.domain, domain);
assert.strictEqual(typeof stream.emit, "function");
domain.on("error", (error) => {
  assert.match(error.message, /^ENOENT: no such file or directory, open '/);
  assert.strictEqual(error.domain, domain);
  assert.strictEqual(error.domainEmitter, stream);
});
