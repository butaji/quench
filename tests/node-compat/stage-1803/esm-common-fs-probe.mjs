import {
  assertDirEquivalent,
  nextdir,
} from "../../../tests/node/test/common/fs.js";
import fixtures from "../../../tests/node/test/common/fixtures.js";
import { constants, lstatSync } from "node:fs";

if (typeof nextdir !== "function") throw new Error("nextdir missing");
if (typeof assertDirEquivalent !== "function") {
  throw new Error("assertDirEquivalent missing");
}
if (typeof fixtures.path !== "function") {
  throw new Error("fixtures.path missing");
}
if (constants.COPYFILE_FICLONE_FORCE !== 4) {
  throw new Error("fs constants missing");
}
if (typeof lstatSync !== "function") throw new Error("lstatSync missing");
if (typeof nextdir() !== "string") throw new Error("nextdir result missing");
