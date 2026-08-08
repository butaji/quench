const assert = require("assert");
const { Readable } = require("stream");

const readable = new Readable({ read() {} });
readable.push("queued");
readable.destroy();
readable.on("data", () => assert.fail("destroyed stream emitted queued data"));

console.log("stream destroy data flush pass");
