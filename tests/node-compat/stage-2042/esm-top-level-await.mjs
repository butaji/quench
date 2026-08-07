const result = await Promise.resolve("settled");
if (result !== "settled") throw new Error("top-level await did not settle");
console.log("esm top-level await passed");
