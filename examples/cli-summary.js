const path = require("node:path");

console.log(JSON.stringify({
  cwd: process.cwd(),
  basename: path.basename(process.argv[1]),
  arguments: process.argv.slice(2),
}));
