const { types } = require("util");

if (
  !types.isDate(new Date()) ||
  !types.isMap(new Map()) ||
  !types.isArrayBufferView(new Uint8Array())
) {
  throw new Error("util.types predicates failed");
}
