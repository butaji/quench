const { Buffer } = require("buffer");
const { internalBinding } = require("internal/test/binding");

const debug = internalBinding("debug");
const before = debug.getGenericUsageCount(
  "NodeArrayBufferAllocator.Allocate.Uninitialized",
);
Buffer.allocUnsafe(8);
const after = debug.getGenericUsageCount(
  "NodeArrayBufferAllocator.Allocate.Uninitialized",
);
if (after <= before) throw new Error("allocator counter did not advance");
