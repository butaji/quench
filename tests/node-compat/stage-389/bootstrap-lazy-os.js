if (globalThis.__nodeOsInitialized !== false) {
  throw new Error("os compatibility initialized during bootstrap");
}

const os = require("os");
if (
  globalThis.__nodeOsInitialized !== true ||
  typeof os.platform !== "function"
) {
  throw new Error("os compatibility did not initialize on access");
}

console.log("lazy os bootstrap passed");
