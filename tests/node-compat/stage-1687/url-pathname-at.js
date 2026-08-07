const url = new URL("http::@c:29", "http://example.org/foo/bar");
if (url.href !== "http://example.org/foo/:@c:29") {
  throw new Error(`Unexpected URL: ${url.href}`);
}
console.log("URL pathnames preserve at signs");
