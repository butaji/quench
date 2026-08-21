const assert = require("assert");

const frame = require("internal/async_context_frame");
const hooks = require("internal/async_hooks");
assert.strictEqual(frame.current(), null);
assert.strictEqual(hooks.enabledHooksExist(), false);
