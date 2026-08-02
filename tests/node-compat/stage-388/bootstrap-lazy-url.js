if (globalThis.__nodeUrlInitialized !== false) {
  throw new Error("url compatibility initialized during bootstrap");
}

const url = require("url");
if (
  globalThis.__nodeUrlInitialized !== true ||
  typeof url.parse !== "function"
) {
  throw new Error("url compatibility did not initialize on access");
}

console.log("lazy url bootstrap passed");
