const { internalBinding } = require("internal/test/binding");

const view = new Uint8Array(48);
const check = internalBinding("util").arrayBufferViewHasBuffer;
if (check(view) || !check(view)) throw new Error("lazy backing state failed");
