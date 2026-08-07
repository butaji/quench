const url = require("url");

const formatted = url.format("http://example.com?foo=@bar#frag");
if (formatted !== "http://example.com/?foo=@bar#frag") {
  throw new Error(`legacy query was encoded: ${formatted}`);
}
