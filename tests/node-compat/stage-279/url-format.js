const { format } = require("url");

if (format("http://example.com?") !== "http://example.com/?") {
  throw new Error("empty query marker was lost");
}
if (
  format({
    protocol: "http:",
    slashes: true,
    host: "example.com",
    pathname: "/",
    search: "?x=1",
  }) !== "http://example.com/?x=1"
) {
  throw new Error("legacy URL object formatting failed");
}
