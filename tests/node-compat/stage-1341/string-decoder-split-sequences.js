const assert = require("node:assert");
const { StringDecoder } = require("node:string_decoder");

const writeSequences = (length, start = 0, sequence = []) => {
  if (start === length) return [sequence];
  const result = [];
  for (let end = length; end > start; end--) {
    result.push(
      ...writeSequences(length, end, sequence.concat([[start, end]])),
    );
  }
  return result;
};
const check = (encoding, input, expected) => {
  for (const sequence of writeSequences(input.length)) {
    const decoder = new StringDecoder(encoding);
    let output = "";
    for (const [start, end] of sequence) {
      output += decoder.write(input.slice(start, end));
    }
    output += decoder.end();
    assert.strictEqual(output, expected, JSON.stringify(sequence));
  }
};

check("utf-8", Buffer.from("$"), "$ ".trim());
check("utf-8", Buffer.from("¢"), "¢");
check("utf-8", Buffer.from("€"), "€");
check("utf-8", Buffer.from("𤭢"), "𤭢");
check("utf-8", Buffer.from("C9B5A941", "hex"), "\u0275�A");
check("utf-8", Buffer.from("E241", "hex"), "�A");
check("utf-16le", Buffer.from("3DD84DDC", "hex"), "👍");
console.log("string decoder split sequences passed");
