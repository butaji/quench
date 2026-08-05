const assert = require("assert");
const { Readable } = require("stream");

const stream = new Readable({ read() {} });
const values = stream.drop(1).take(1).toArray();
stream.emit("data", 1);
stream.emit("data", 2);
stream.emit("end");

values.then((result) => assert.deepStrictEqual(result, [2]));
