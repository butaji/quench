const common = require("../common");
const buffer = Buffer.from("abc");
const views = common.getArrayBufferViews(buffer);
if (views.length < 3 || views.some((view) => view.byteLength !== 3))
  throw new Error("common views mismatch");
