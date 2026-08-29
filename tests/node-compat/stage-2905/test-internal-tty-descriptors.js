const assert = require("assert");
const { internalBinding } = require("internal/test/binding");

const tty = internalBinding("tty_wrap").TTY;
for (const key of ["bytesRead", "fd", "_externalStream"]) {
  assert.strictEqual(Object.prototype.propertyIsEnumerable.call(tty, key), false);
}
console.log("internal tty descriptors passed");
