const readline = require("readline");
const input = readline.createInterface({ input: process.stdin });
let count = 0;
input.on("line", (line) => { count += line.length; });
input.on("close", () => console.log(count));
