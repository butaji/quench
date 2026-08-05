const assert = require("assert");
const fs = require("fs");

const stream = fs.createReadStream(__filename);
stream.on("close", () => assert.strictEqual(stream.fd, null));
stream.resume();
