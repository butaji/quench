// Stage 33 tracks the first upstream Node test dependency: common/tmpdir.
const tmpdir = require("../common/tmpdir");
tmpdir.refresh();
if (!tmpdir.resolve("fixture").startsWith("/tmp/")) {
  throw new Error("tmpdir helper");
}
