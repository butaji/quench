if (globalThis.__nodeQuerystringInitialized !== false) {
  throw new Error("querystring initialized during bootstrap");
}

const querystring = require("querystring");
if (
  globalThis.__nodeQuerystringInitialized !== true ||
  querystring.stringify({ value: "ok" }) !== "value=ok"
) {
  throw new Error("querystring did not initialize on access");
}

console.log("lazy querystring bootstrap passed");
