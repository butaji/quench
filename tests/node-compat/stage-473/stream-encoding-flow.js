const { Readable } = require("stream");

const stream = new Readable();
const values = [];
stream.setEncoding("utf8");
stream.on("data", (value) => values.push(value));
stream.push(Buffer.from("flowing"));

if (values.length !== 1 || values[0] !== "flowing") {
  throw new Error("flowing data was not decoded");
}

console.log("stream encoding flow passed");
