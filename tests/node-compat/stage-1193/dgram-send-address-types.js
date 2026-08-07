const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
for (const address of [[], 0, 1, true, false, 0n, 1n, {}, Symbol()]) {
  assert.throws(() => socket.send(Buffer.from("x"), 40000, address), {
    code: "ERR_INVALID_ARG_TYPE",
    name: "TypeError",
    message: `The "address" argument must be of type string.${
      Array.isArray(address)
        ? " Received an instance of Array"
        : typeof address === "object"
        ? " Received an instance of Object"
        : ` Received type ${typeof address} (${String(address)}${
          typeof address === "bigint" ? "n" : ""
        })`
    }`,
  });
}
socket.close();
