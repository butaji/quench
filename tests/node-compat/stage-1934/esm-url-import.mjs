import assert from "node:assert";
import { fileURLToPath, pathToFileURL } from "node:url";

const url = pathToFileURL("/tmp/quench-node-url-probe");
assert.strictEqual(fileURLToPath(url), "/tmp/quench-node-url-probe");
console.log("esm url imports passed");
