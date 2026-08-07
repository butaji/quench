import { createRequire } from "module";
if (typeof createRequire !== "function") {
  throw new Error(`createRequire=${typeof createRequire}`);
}
console.log("esm module createRequire passed");
