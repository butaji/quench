const { types } = require("util");

const buffer = new ArrayBuffer();
const dataView = new DataView(buffer);
const stealthyDataView = Object.setPrototypeOf(
  new DataView(buffer),
  Uint8Array.prototype,
);
const typedArray = new Uint8Array(buffer);

if (!types.isArrayBufferView(dataView)) throw new Error("DataView view check");
if (types.isTypedArray(dataView)) throw new Error("DataView typed check");
if (!types.isArrayBufferView(stealthyDataView)) {
  throw new Error("stealthy DataView view check");
}
if (types.isTypedArray(stealthyDataView)) {
  throw new Error("stealthy DataView typed check");
}
if (!types.isTypedArray(typedArray)) throw new Error("typed array check");
