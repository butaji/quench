const fs = require("node:fs");
if (typeof fs.cp !== "function") throw new Error(`fs.cp=${typeof fs.cp}`);
console.log("fs cp cjs surface passed");
