const { internalBinding } = require("internal/test/binding");

const { arrayBufferViewHasBuffer } = internalBinding("util");
if (arrayBufferViewHasBuffer(new Uint8Array(48))) {
  throw new Error("small view incorrectly marked backed");
}
if (!arrayBufferViewHasBuffer(new Uint8Array(96))) {
  throw new Error("large view missing backing marker");
}
