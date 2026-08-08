const assert = require("assert");

const original = process.stdout.write;
const output = [];
process.stdout.write = (value) => {
  output.push(String(value));
  return true;
};

console.log("req: %s headers: %j", "HEAD", { "Content-Length": 12 });
process.stdout.write = original;

assert.deepStrictEqual(output, ['req: HEAD headers: {"Content-Length":12}\n']);
console.log("console format passed");
