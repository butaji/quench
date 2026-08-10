const net = require("net");

if (net.getDefaultAutoSelectFamily() !== false) {
  throw new Error("unexpected net auto-select-family default");
}
if (net.getDefaultAutoSelectFamilyAttemptTimeout() !== 2500) {
  throw new Error("unexpected net auto-select-family timeout");
}
if (typeof atob !== "function" || typeof btoa !== "function") {
  throw new Error("base64 web globals are unavailable");
}
