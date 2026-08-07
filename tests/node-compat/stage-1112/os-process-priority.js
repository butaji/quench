const { constants, getPriority, setPriority } = require("os");
const { PRIORITY_NORMAL, PRIORITY_HIGHEST } = constants.priority;
if (typeof PRIORITY_HIGHEST !== "number") {
  throw new Error("missing priority constant");
}
for (const pid of [null, true, "foo", {}, []]) {
  try {
    getPriority(pid);
    throw new Error("invalid pid accepted");
  } catch (error) {
    if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
  }
}
setPriority(0, PRIORITY_NORMAL);
if (getPriority(0) !== PRIORITY_NORMAL) {
  throw new Error("priority was not stored");
}
