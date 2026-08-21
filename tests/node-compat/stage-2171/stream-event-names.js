const assert = require("assert");
const { Readable, Writable, Duplex } = require("stream");

const readable = new Readable();
readable.on("foo", () => {});
readable.on("data", () => {});
readable.on("error", () => {});
assert.deepStrictEqual(readable.eventNames(), ["error", "data", "foo"]);

const writable = new Writable();
writable.on("foo", () => {});
writable.on("drain", () => {});
writable.on("prefinish", () => {});
assert.deepStrictEqual(writable.eventNames(), ["prefinish", "drain", "foo"]);

const duplex = new Duplex();
duplex.on("foo", () => {});
duplex.on("finish", () => {});
assert.deepStrictEqual(duplex.eventNames(), ["finish", "foo"]);

console.log("stream event names pass");
