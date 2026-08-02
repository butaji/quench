const { randomBytes, randomFillSync } = require("crypto");

const bytes = randomBytes(4);
if (!(bytes instanceof Uint8Array) || bytes.length !== 4) {
  throw new Error("randomBytes failed");
}
const target = new Uint8Array(3);
if (randomFillSync(target) !== target) throw new Error("randomFillSync failed");
