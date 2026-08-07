const utilBinding = process.binding("util");
const expected = [
  "isAnyArrayBuffer",
  "isArrayBuffer",
  "isArrayBufferView",
  "isAsyncFunction",
  "isDataView",
  "isDate",
  "isExternal",
  "isMap",
  "isMapIterator",
  "isNativeError",
  "isPromise",
  "isRegExp",
  "isSet",
  "isSetIterator",
  "isTypedArray",
  "isUint8Array",
];
if (
  JSON.stringify(Object.keys(utilBinding).sort()) !== JSON.stringify(expected)
) {
  throw new Error("unexpected process.binding('util') keys");
}
for (const name of expected) {
  if (
    utilBinding[name] !== require("util").types[name] &&
    name !== "isExternal"
  ) {
    throw new Error(`binding mismatch: ${name}`);
  }
}
