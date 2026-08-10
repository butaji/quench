const assert = require("assert");
const { PassThrough, Readable } = require("stream");

const source = Readable.from([1, 2, 3]);
const first = new PassThrough({ objectMode: true });
const output = source.pipe(first).pipe(
  new PassThrough({ objectMode: true }),
);
const values = [];
output.on("end", () => {
  assert.deepStrictEqual(values, [1, 2, 3]);
  console.log("pass-through buffered read end passed");
});
const read = () => {
  let value;
  while ((value = output.read()) !== null) values.push(value);
  if (!output.readableEnded) output.once("readable", read);
};
output.on("readable", read);
