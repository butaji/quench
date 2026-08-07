const { Readable } = require("stream");
const checks = [];
const record = (name, value) => {
  checks.push(`${name}:${value}`);
  if (!value) throw new Error(name);
};
const first = new Readable({ highWaterMark: 3 });
first._read = () => {
  throw new Error("first read");
};
first.push(Buffer.from("blerg"));
setTimeout(() => record("first-not-reading", !first._readableState.reading), 1);

const second = new Readable({ highWaterMark: 3 });
second._read = () => {};
second.push(Buffer.from("bl"));
setTimeout(() => record("second-reading", second._readableState.reading), 1);

const third = new Readable({ highWaterMark: 30 });
third._read = () => {
  throw new Error("third read");
};
third.push(Buffer.from("blerg"));
third.push(null);
setTimeout(() => record("third-not-reading", !third._readableState.reading), 1);

const values = [];
const source = ["", "x", "y", "", "z"];
const fourth = new Readable({ encoding: "utf8" });
fourth._read = () =>
  queueMicrotask(() => {
    if (!source.length) fourth.push(null);
    else fourth.push(source.shift());
  });
fourth.on("readable", () => {
  const value = fourth.read();
  if (value !== null) values.push(value);
});
fourth.on("end", () =>
  record(
    "fourth-values",
    JSON.stringify(values) === JSON.stringify(["x", "y", "z"]),
  ));
