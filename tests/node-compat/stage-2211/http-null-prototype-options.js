const assert = require("assert");
const http = require("http");

const options = { __proto__: null, host: "localhost", port: 1, path: "/" };
Object.defineProperty(Object.prototype, "hostname", {
  configurable: true,
  get() {
    throw new Error("inherited hostname was accessed");
  },
});

assert.doesNotThrow(() => {
  const request = http.request(options);
  request.on("error", () => {});
  request.destroy();
});
delete Object.prototype.hostname;
