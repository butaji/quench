const { Buffer } = require("buffer");

const actual = Buffer.from("hérité").toString("ascii");
if (actual !== "hC)ritC)") {
  throw new Error(`ascii result ${JSON.stringify(actual)}`);
}

const input =
  "C’est, graphiquement, la réunion d’un accent aigu et d’un accent grave.";
const expected =
  "Cb\u0000\u0019est, graphiquement, la rC)union db\u0000\u0019un accent aigu et db\u0000\u0019un accent grave.";
const output = Buffer.from(input).toString("ascii");
if (output !== expected) throw new Error("long ascii conversion failed");
