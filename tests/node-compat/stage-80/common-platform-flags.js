const common = require("../common");

if (common.isWindows !== (process.platform === "win32")) {
  throw new Error("isWindows mismatch");
}
if (common.isAIX || common.isFreeBSD) {
  throw new Error("unexpected platform flag");
}
