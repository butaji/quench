const os = require("os");
if (os.EOL !== "\n") throw new Error(`unexpected default EOL: ${os.EOL}`);
Object.defineProperties(os, {
  EOL: { configurable: true, enumerable: true, writable: false, value: "foo" },
});
if (os.EOL !== "foo") throw new Error("os.EOL override was ignored");
