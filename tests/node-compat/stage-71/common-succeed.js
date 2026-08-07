const common = require("../common");
const tmpdir = require("../common/tmpdir");

tmpdir.refresh();
if (!tmpdir.path.startsWith("/")) {
  throw new Error("tmpdir.path must be absolute");
}
common.mustSucceed((value) => {
  if (value !== "ok") throw new Error("mustSucceed did not forward data");
})(null, "ok");
