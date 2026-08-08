const { Readable, PassThrough } = require("stream");
const source = new Readable({ objectMode: true, read() {} });
const target = source.pipe(
  new PassThrough({ objectMode: true, highWaterMark: 2 })
);
if (target.listenerCount("drain") !== 0) throw new Error("unexpected drain");
source.push("asd");
if (target.listenerCount("drain") !== 0) throw new Error("unexpected drain");
