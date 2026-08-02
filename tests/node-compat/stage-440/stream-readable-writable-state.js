const { Readable, Writable } = require("stream");

const readable = Readable.from(["value"]);
if (!readable.readable) throw new Error("readable flag was not set");
readable.destroy();
if (readable.readable) throw new Error("destroy did not clear readable");

const writable = new Writable();
if (!writable.writable) throw new Error("writable flag was not set");
writable.destroy();
if (writable.writable) throw new Error("destroy did not clear writable");

console.log("stream readable writable state passed");
