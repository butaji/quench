const url = new URL("ws:/example.com/");
if (url.href !== "ws://example.com/") {
  throw new Error(`Unexpected URL: ${url.href}`);
}
console.log("URL WebSocket single-slash schemes are normalized");
