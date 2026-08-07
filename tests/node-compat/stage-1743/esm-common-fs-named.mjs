import { nextdir } from "../../../tests/node/test/common/fs.js";
if (typeof nextdir !== "function") throw new Error("nextdir missing");
console.log("esm common fs named passed");
