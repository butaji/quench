const { TextEncoder, TextDecoder } = require("util");

const encoded = new TextEncoder().encode("hello");
if (new TextDecoder().decode(encoded) !== "hello") {
  throw new Error("util encoder exports failed");
}
