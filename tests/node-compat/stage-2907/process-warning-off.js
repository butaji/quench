const assert = require("assert");
let a = 0;
const first = () => { a++; };
const second = () => { a += 10; };
process.on("warning", first);
process.off("warning", first);
process.on("warning", second);
process.emit("warning", { name: "Warning", message: "x" });
assert.strictEqual(a, 10);
