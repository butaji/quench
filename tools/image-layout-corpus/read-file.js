const fs = require("fs");
const text = fs.readFileSync(process.argv[2] || __filename, "utf8");
console.log(text.split("\n").length + " lines");
