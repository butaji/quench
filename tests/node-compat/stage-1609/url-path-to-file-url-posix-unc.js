const assert = require("node:assert");
const url = require("node:url");

const unc = url.pathToFileURL("\\\\nas\\share\\path.txt").href;
assert.match(unc, /file:\/\/.+%5C%5Cnas%5Cshare%5Cpath\.txt$/);
assert.ok(url.pathToFileURL("test/").href.endsWith("/"));
assert.ok(url.pathToFileURL("test\\").href.endsWith("%5C"));
assert.ok(url.pathToFileURL("test/%").href.includes("%25"));
console.log("POSIX UNC file URL matrix passed");
