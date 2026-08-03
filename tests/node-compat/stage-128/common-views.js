const common = require("../common");
const assert = require("assert");
const buffer = Buffer.from("abc");
const views = common.getArrayBufferViews(buffer);
assert.ok(views.length >= 3);
assert.ok(views.every((view) => view.byteLength === 3));
