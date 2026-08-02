const { internalBinding } = require("internal/test/binding");

const stream = new (internalBinding("js_stream").JSStream)();
if (!stream._externalStream) throw new Error("JSStream shim failed");
