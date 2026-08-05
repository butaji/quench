const assert = require("assert");
const { Readable } = require("stream");

const stream = new Readable({ read() {} });
const result = stream.map((value) => value * 2).filter((value) => value > 2);
const values = result.toArray();
stream.emit("data", 1);
stream.emit("data", 2);
stream.emit("end");

values.then((items) => assert.deepStrictEqual(items, [4]));
